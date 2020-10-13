# WebComputing for Golem - Web Server

This rust implementation of the web server, which is the part of WebComputing for Golem project.

In order to run it, simply, execute the command in server directory:
```
cargo run
```
The server listens on `http://localhost:8080`. To change the endpoint address, modify the code of `main.rs`. In the future it should be a parameter.

The client is web browser based. Just open
```
http://localhost:8080/
```
which displays the static page `index.html`. You can click the button `check for a new task` on the page. 
This checks whether there are tasks available on the server or not and if it is the case, computes a task.

All task data, input files and resulting files, are stored in `server/task` directory. They are not being removed. They will be in the future.

Tasks statuses are in memory. So at the moment, tasks are lost when the server is restarted.

`hello-wasi.wasm` is an examplary wasm binary for a task.

Tested with `1.46` rust.

## Web Methods for gFaaS

There are four methods for `gFaaS`.

The post method `createTask` creates a new task in the server. There is a static page `uploadfile.html` for testing. Open it directly from a disk. 
It can be used to call the method and create a new task manually.

The get method `taskStatus/{task_id}` returns a task status. A task id is a random alphanumeric 20 char string.
```
curl http://localhost:8080/taskStatus/ErnD1ZnBHtcyrs9AlXO2
```

The get method `taskResult/{task_id}` returns a list of result files if the task is finished.
```
curl http://localhost:8080/taskResult/ErnD1ZnBHtcyrs9AlXO2
```

The get method `/taskResult/{task_id}/{file_name}` sends a single file from result files.
```
curl http://localhost:8080/taskResult/ErnD1ZnBHtcyrs9AlXO2/output
```

So the flow is as below
1. `createTask` and you get `task_id`
2. `taskStatus` until you get `Completed` (of `Gone` or `Failed`)
3. `taskResult` without a file name to get a list of result files
4. `taskResult` with a file name for every result file from the list
