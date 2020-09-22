use actix_files::NamedFile;
use actix_web::{get, post, web, App, HttpServer, HttpResponse, Responder, Result, Error};
use serde::Deserialize;
use async_std::prelude::*;
use futures::{StreamExt, TryStreamExt};
use actix_multipart::Multipart;
use rand::{thread_rng, Rng};
use rand::distributions::Alphanumeric;
use std::fs;
use std::collections::HashMap;
use lazy_static::lazy_static;
use std::sync::Mutex;
use std::fmt::Formatter;
use std::fmt::Display;
use std::path::Path;

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

#[derive(Deserialize)]
struct TaskIdForm {
    task_id: String,
}

//test
#[get("/hello")]
async fn hello() -> impl Responder {
    "Hello there!"
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
    let mut tasks_map = TASKS.lock().unwrap();

    let mut d = String::new();
    for key in tasks_map.keys() {
        let value = tasks_map.get(key);
        match value {
            Some(v) => { 
                if *v == TaskStatus::Enqueued {
                    d = key.clone();
                    break;
                }
            }
            None => continue
        }
    }
    if d.is_empty() {
        return Ok(HttpResponse::Ok()
            .content_type("application/json")
            .body("{\"result\":\"0\"}"));
    }

    //we need to find .wasm file
    let task_input_dir = format!("task/{}/input", d);
    let task_input_path = Path::new(&task_input_dir);
    if !task_input_path.is_dir() {
        //this task has corrupted directory, the task is invalid
        tasks_map.insert(d.clone(), TaskStatus::Failed);
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
            Some(e) => 
                if (*e).to_str().unwrap() == "wasm" {
                    tasks_map.insert(d.clone(), TaskStatus::Sent);
                    return Ok(HttpResponse::Ok()
                        .content_type("application/json")
                        .body(format!("{{\"result\":\"1\", \"taskId\":\"{}\", \"fileName\":\"{}\"}}", d, file_name.to_str().unwrap())));
                }
            None => continue
        }
    }

    //this task does not have .wasm file
    tasks_map.insert(d.clone(), TaskStatus::Failed);
    return Ok(HttpResponse::Ok()
        .content_type("application/json")
        .body("{\"result\":\"0\"}"));

}

//a client completed a task
#[post("/taskDone/{task_id}")]
async fn task_done((web::Path(task_id), mut payload): (web::Path<String>, Multipart)) -> Result<HttpResponse, Error> {
    let mut tasks_map = TASKS.lock().unwrap();
    let status = tasks_map.get(&task_id);
    match status {
        Some(s) => {
            if *s == TaskStatus::Sent {
                tasks_map.insert(task_id.clone(), TaskStatus::Completed);
                fs::create_dir(format!("./task/{}/result", &task_id))?;
                // iterate over multipart stream
                while let Ok(Some(mut field)) = payload.try_next().await {
                    let content_type = field
                        .content_disposition()
                        .ok_or_else(|| actix_web::error::ParseError::Incomplete)?;
                    let filename = content_type
                        .get_filename()
                        .ok_or_else(|| actix_web::error::ParseError::Incomplete)?;
                    let filepath = format!("./task/{}/result/{}", &task_id, sanitize_filename::sanitize(&filename));
                    let mut f = async_std::fs::File::create(filepath).await?;
                    // Field in turn is stream of *Bytes* object
                    while let Some(chunk) = field.next().await {
                        let data = chunk.unwrap();
                        f.write_all(&data).await?;
                    }
                }
                Ok(HttpResponse::Ok().content_type("text/plain").body("ok"))
            } else {
                Ok(HttpResponse::Conflict().finish())
            }
        }
        None => Ok(HttpResponse::NotFound().finish())
    }
}

//a client downloads files for a task
#[get("/task/{task_id}/{file_name}")]
async fn task_file(web::Path((task_id, file_name)): web::Path<(String, String)>) -> Result<NamedFile> {
    Ok(NamedFile::open(format!("task/{}/input/{}", task_id, file_name))?)
}

//gFaaS checks a task status
#[get("/taskStatus/{task_id}")]
async fn task_status(web::Path(task_id): web::Path<String>) -> Result<HttpResponse> {
    let tasks_map = TASKS.lock().unwrap();
    let task_status = tasks_map.get(&task_id);

    match task_status {
        Some(s) =>  Ok(HttpResponse::Ok().content_type("text/plain").body(s.to_string())),
        None => Ok(HttpResponse::Ok().content_type("text/plain").body(TaskStatus::Gone.to_string())),
    }
}

//gFaaS creates a new task
#[post("/createTask")]
async fn create_task(mut payload: Multipart) -> Result<HttpResponse, Error> {
    let rand_string: String = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(20)
        .collect();
    fs::create_dir(format!("./task/{}", rand_string))?;
    fs::create_dir(format!("./task/{}/input", rand_string))?;

    // iterate over multipart stream
    while let Ok(Some(mut field)) = payload.try_next().await {
        let content_type = field
            .content_disposition()
            .ok_or_else(|| actix_web::error::ParseError::Incomplete)?;
        let filename = content_type
            .get_filename()
            .ok_or_else(|| actix_web::error::ParseError::Incomplete)?;
        let filepath = format!("./task/{}/input/{}", rand_string, sanitize_filename::sanitize(&filename));
        let mut f = async_std::fs::File::create(filepath).await?;

        // Field in turn is stream of *Bytes* object
        while let Some(chunk) = field.next().await {
            let data = chunk.unwrap();
            f.write_all(&data).await?;
        }
    }

    let task_id = rand_string.clone();
    let mut tasks_map = TASKS.lock().unwrap();
    tasks_map.insert(task_id, TaskStatus::Enqueued);

    Ok(HttpResponse::Ok().content_type("text/plain")
        .body(rand_string))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(|| App::new()
            .service(hello)
            .service(wasm)
            .service(index)
            .service(check_for_task)
            .service(task_done)
            .service(task_file)
            .service(task_status)
            .service(create_task))
        .bind("127.0.0.1:8080")?
        .run()
        .await
}