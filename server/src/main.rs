use actix_files::NamedFile;
use actix_multipart::Multipart;
use actix_web::{get, post, web, App, Error, HttpResponse, HttpServer, Responder, Result};
use async_std::prelude::*;
use futures::{StreamExt, TryStreamExt};
use lazy_static::lazy_static;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::collections::HashMap;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fs;
use std::path::Path;
use std::sync::Mutex;

#[derive(PartialEq, Clone, Copy)]
enum TaskStatus {
    Enqueued,
    Sent,
    Completed,
    Failed,
    Gone,
}
impl Display for TaskStatus {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match *self {
            TaskStatus::Enqueued => write!(f, "Enqueued"),
            TaskStatus::Sent => write!(f, "Sent"),
            TaskStatus::Completed => write!(f, "Completed"),
            TaskStatus::Failed => write!(f, "Failed"),
            TaskStatus::Gone => write!(f, "Gone"),
        }
    }
}

lazy_static! {
    static ref TASKS: Mutex<HashMap<String, TaskStatus>> = Mutex::new(HashMap::new());
}

//test
#[get("/hello")]
async fn hello() -> impl Responder {
    "Hello there!"
}

#[get("/favicon.ico")]
async fn favicon() -> Result<NamedFile> {
    Ok(NamedFile::open("favicon-32x32.png")?)
}

#[get("/golemlab.png")]
async fn golemlab_logo() -> Result<NamedFile> {
    Ok(NamedFile::open("golemlab.png")?)
}

#[get("/background.png")]
async fn background() -> Result<NamedFile> {
    Ok(NamedFile::open("01background.png")?)
}

//main page for a client
#[get("/")]
async fn index() -> Result<NamedFile> {
    Ok(NamedFile::open("index.html")?)
}

//test
#[get("/hello-wasi.wasm")]
async fn wasm() -> Result<NamedFile> {
    Ok(NamedFile::open("hello-wasi.wasm")?)
}

//a client wants a new task
#[post("/checkForTask")]
async fn check_for_task() -> Result<HttpResponse> {
    let task_id = {
        let mut tasks_map = TASKS.lock().unwrap();
        let mut out = String::new();
        for key in tasks_map.keys() {
            let value = tasks_map.get(key);
            match value {
                Some(v) => if *v == TaskStatus::Enqueued  {
                    out = key.clone();
                    tasks_map.insert(out.clone(), TaskStatus::Sent);
                    break;
                }
                None => continue
            }
        }
        out
    };

    if task_id.is_empty() {
        return Ok(HttpResponse::Ok()
            .content_type("application/json")
            .body("{\"result\":\"0\"}"));
    }

    //we need to find .wasm file
    let task_input_dir = format!("task/{}/input", task_id);
    let task_input_path = Path::new(&task_input_dir);
    if !task_input_path.is_dir() {
        //this task has corrupted directory, the task is invalid
        TASKS.lock().unwrap().insert(task_id, TaskStatus::Failed);
        return Ok(HttpResponse::Ok()
            .content_type("application/json")
            .body("{\"result\":\"0\"}"));
    }
    for entry in fs::read_dir(task_input_path)? {
        let entry = entry?;
        if !entry.path().is_file() {
            continue;
        }
        let file_name = entry.file_name();
        let file_path = entry.path();
        let file_ext = file_path.extension();
        match file_ext {
            Some(e) => {
                if (*e).to_str() == Some("wasm") {
                    return Ok(HttpResponse::Ok()
                        .content_type("application/json")
                        .body(format!(
                            "{{\"result\":\"1\", \"taskId\":\"{}\", \"fileName\":\"{}\"}}",
                            task_id,
                            file_name.to_str().unwrap()
                        )));
                }
            }
            None => continue,
        }
    }

    //this task does not have .wasm file
    TASKS.lock().unwrap().insert(task_id, TaskStatus::Failed);
    return Ok(HttpResponse::Ok()
        .content_type("application/json")
        .body("{\"result\":\"0\"}"));
}

//a client completed a task
#[post("/taskDone/{task_id}")]
async fn task_done(
    (web::Path(task_id), mut payload): (web::Path<String>, Multipart),
) -> Result<HttpResponse, Error> {
    {
        let tasks_map = TASKS.lock().unwrap();
        let status = tasks_map.get(&task_id);
        match status {
            Some(s) => {
                if *s != TaskStatus::Sent {
                    return Ok(HttpResponse::Conflict().finish());
                }
            }
            None => return Ok(HttpResponse::NotFound().finish()),
        }
    }

    fs::create_dir(Path::new("task").join(&task_id).join("result"))?;
    // iterate over multipart stream
    while let Some(mut field) = payload.try_next().await? {
        let content_type = field
                        .content_disposition()
                        .ok_or_else(|| actix_web::error::ParseError::Incomplete)?;
        let filename = content_type
                        .get_filename()
                        .ok_or_else(|| actix_web::error::ParseError::Incomplete)?;
        let filepath = Path::new("task")
                        .join(&task_id)
                        .join("result")
                        .join(sanitize_filename::sanitize(&filename));
        let mut f = async_std::fs::File::create(filepath).await?;
        // Field in turn is stream of *Bytes* object
        while let Some(chunk) = field.next().await {
            let data = chunk?;
            f.write_all(&data).await?;
        }
    }
    TASKS.lock().unwrap().insert(task_id.clone(), TaskStatus::Completed);
    Ok(HttpResponse::Ok().content_type("text/plain").body("ok"))
}

//a client downloads files for a task
#[get("/task/{task_id}/{file_name}")]
async fn task_file(web::Path((task_id, file_name)): web::Path<(String, String)>,) -> Result<NamedFile> {
    Ok(NamedFile::open(
        Path::new("task")
            .join(task_id)
            .join("input")
            .join(file_name),
    )?)
}

//gFaaS downloads output files
#[get("/taskResult/{task_id}/{file_name}")]
async fn task_result_file(web::Path((task_id, file_name)): web::Path<(String, String)>,) -> Result<NamedFile> {
    Ok(NamedFile::open(
        Path::new("task")
            .join(task_id)
            .join("result")
            .join(file_name),
    )?)
}

//gFaaS reads a list of output files
#[get("/taskResult/{task_id}")]
async fn task_result(web::Path(task_id): web::Path<String>) -> Result<HttpResponse> {
    {
        let tasks_map = TASKS.lock().unwrap();
        let status = tasks_map.get(&task_id);
        match status {
            Some(s) => {
                if *s != TaskStatus::Completed {
                    return Ok(HttpResponse::Conflict().finish());
                }
            }
            None => return Ok(HttpResponse::NotFound().finish()),
        }
    }

    let task_result_dir = format!("task/{}/result", task_id);
    let task_result_path = Path::new(&task_result_dir);
    if !task_result_path.is_dir() {
        return Ok(HttpResponse::InternalServerError()
            .content_type("plain/text")
            .body("no result directory"));
    }
    
    let mut response = String::from("{\"output_files\":[");
    let mut first = true;

    for entry in fs::read_dir(task_result_path)? {
        let entry = entry?;
        if !entry.path().is_file() {
            continue;
        }
        let file_name = entry.file_name();
        response.push('"');
        response.push_str(file_name.to_str().unwrap());
        response.push('"');
        if first {
            first = false;
        } else {
            response.push(',');
        }
    }

    response.push_str("]}");

    return Ok(HttpResponse::Ok()
        .content_type("application/json")
        .body(response));

}

//gFaaS checks a task status
#[get("/taskStatus/{task_id}")]
async fn task_status(web::Path(task_id): web::Path<String>) -> Result<HttpResponse> {
    let tasks_map = TASKS.lock().unwrap();
    let task_status = tasks_map.get(&task_id);

    match task_status {
        Some(s) => Ok(HttpResponse::Ok()
            .content_type("text/plain")
            .body(s.to_string())),
        None => Ok(HttpResponse::Ok()
            .content_type("text/plain")
            .body(TaskStatus::Gone.to_string())),
    }
}

//gFaaS creates a new task
#[post("/createTask")]
async fn create_task(mut payload: Multipart) -> Result<HttpResponse, Error> {
    let rand_string: String = thread_rng().sample_iter(&Alphanumeric).take(20).collect();
    fs::create_dir(Path::new("task").join(&rand_string))?;
    fs::create_dir(Path::new("task").join(&rand_string).join("input"))?;

    // iterate over multipart stream
    while let Some(mut field) = payload.try_next().await? {
        let content_type = field
            .content_disposition()
            .ok_or_else(|| actix_web::error::ParseError::Incomplete)?;
        let filename = content_type
            .get_filename()
            .ok_or_else(|| actix_web::error::ParseError::Incomplete)?;
        let filepath = format!(
            "./task/{}/input/{}",
            rand_string,
            sanitize_filename::sanitize(&filename)
        );
        let mut f = async_std::fs::File::create(filepath).await?;

        // Field in turn is stream of *Bytes* object
        while let Some(chunk) = field.next().await {
            let data = chunk.unwrap();
            f.write_all(&data).await?;
        }
    }

    let task_id = rand_string.clone();
    TASKS.lock().unwrap().insert(task_id, TaskStatus::Enqueued);

    Ok(HttpResponse::Ok()
        .content_type("text/plain")
        .body(rand_string))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(|| {
        App::new()
            .service(hello)
            .service(wasm)
            .service(index)
            .service(check_for_task)
            .service(task_done)
            .service(task_file)
            .service(task_result_file)
            .service(task_status)
            .service(task_result)
            .service(create_task)
            .service(favicon)
            .service(golemlab_logo)
            .service(background)
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}
