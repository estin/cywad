# CYWAD

A tool automating gathering various numeric values as some kind of metrics from web pages )

[Live demo](https://cywad.herokuapp.com/) with [sources](https://github.com/estin/cywad-demo)

| WARNING: This is a hobby project to learn Rust! It works, but you can implement it in simpler manner, for example, by nodejs + puppeteer |
| --- |

Main features:
 - library or binary modes
 - `cli` mode
 - `server` mode to expose result via OpenAPI 3
 - configurable via TOML files
 - cron-like schedule
 - retry on errors
 - screenshots
 - server-sent events
 - PNG widget for embedding purposes
 - basic authorization using a header or by `?token=` query string parameter

Currently supports next `engines`:

 - embedded browser by [gtk-rs/webkit2gtk-rs](https://github.com/gtk-rs/webkit2gtk-rs)
 - external browser through chrome [devtools](https://chromedevtools.github.io/devtools-protocol/)


## Setup

Currently from sources only.

```bash
$ git clone https://github.com/estin/cywad.git
$ cd cywad
$ cargo build --release --features devtools,server,png_widget
```


## Example - Rust lang stars on github

Create `misc/github-rust.toml` with next content:

```toml

# target url
url = "https://github.com/rust-lang/rust"
# title
name = "rust github stars"

# cron like schedule for server mode
# https://crates.io/crates/cron
cron = "0   *   *     *       *  *  *"

# browser window size
window_width = 1280
window_height = 1024

# timeout to execute one step in ms
step_timeout = 15000

# timeout to wait befor start step in ms
step_interval = 1000

# retry in seconds
retry = [ 60, 300, 600 ]

# Step kind is one of
# - `wait` execute step until result would be `true`
# - `exec` execute code and go to the next step
# - `screenshot` take screenshot
# - `value` get numeric value

# Step0. wait before page is ready
[[steps]]
kind = "wait"
exec = """document.querySelector("div.repohead a[aria-label*='starred']") ? true : false"""

# Step1. get count of stars and paint red border on stars badge
[[steps]]
kind = "value"
key = "stars"
exec = """(() => {
    const el = document.querySelector("div.repohead a[aria-label*='starred']");

    // paint red border
    el.style.borderColor = "red";
    el.style.borderStyle = "solid";

    return parseInt(el.getAttribute("aria-label").split(' ')[0]);
})()"""

    # Determine level of value - any string
    [[steps.levels]]
    name = "green"
    more = 50000
 
    [[steps.levels]]
    name = "red"
    less = 30000 

    [[steps.levels]]
    name = "yellow"
    less = 50000

# Step2. take screenshot
[[steps]]
kind = "screenshot"
key = "stars"
```


## Example of `cli` mode:

Start Chrome devtools with command
```bash
$ chromium --remote-debugging-address=0.0.0.0 --remote-debugging-port=9222 --headless --disable-gpu --disable-software-rasterizer --disable-dev-shm-usage --no-sandbox --enable-logging --allow-running-insecure-content --ignore-certificate-errors
```

```bash
$ cargo run --release --features devtools -- cli -c misc/github-rust.toml
```

Result is
```json
[
  {
    "name": "rust github stars",
    "slug": "rust-github-stars",
    "datetime": "2019-12-21T23:47:34.710012303+03:00",
    "scheduled": null,
    "values": [
      {
        "key": "stars",
        "value": 41332.0,
        "level": "yellow"
      }
    ],
    "error": null,
    "screenshots": [
      {
        "name": "stars",
        "uri": "data:image/png;base64,<...skip...>"
      }
    ],
    "state": "Done",
    "previous": null,
    "steps_done": 3,
    "steps_total": 3,
    "attempt_count": 1
  }
]
```


## Example of `server` mode:


Start server
```bash
$ cargo run --release --features devtools,server -- serve -p misc --listen 127.0.0.1:8000
```

Get info
```bash
$ curl http://127.0.0.1:8000/api/items | jq
  % Total    % Received % Xferd  Average Speed   Time    Time     Time  Current
                                 Dload  Upload   Total   Spent    Left  Speed
100   495  100   495    0     0   483k      0 --:--:-- --:--:-- --:--:--  483k
{
  "info": {
    "name": "cywad",
    "version": "0.1.0",
    "description": "",
    "server_datetime": "2019-12-21T23:52:32.331273021+03:00"
  },
  "items": [
    {
      "name": "rust github stars",
      "slug": "rust-github-stars",
      "datetime": "2019-12-21T23:51:48.608722134+03:00",
      "scheduled": "2019-12-22T08:00:00+03:00",
      "values": [
        {
          "key": "stars",
          "value": 41332,
          "level": "yellow"
        }
      ],
      "error": null,
      "screenshots": [
        {
          "name": "stars",
          "uri": "rust-github-stars/stars.png"
        }
      ],
      "state": "Done",
      "previous": null,
      "steps_done": 3,
      "steps_total": 3,
      "attempt_count": 1
    }
  ]
},
```

Get the screenshot
```bash
$ curl -v http://127.0.0.1:8000/screenshot/rust-github-stars/stars.png 2>&1 | grep "<"
< HTTP/1.1 200 OK
< content-length: 223039
< content-type: image/png
Binary file (standard input) matches
```

You can check out [openapi.yaml](openapi.yaml) for all available rest API methods.

## Testing `devtools` engine

```bash
$ chromium -remote-debugging-address=0.0.0.0 --remote-debugging-port=9222 --headless --disable-gpu --disable-software-rasterizer --disable-dev-shm-usage --no-sandbox --enable-logging --allow
-running-insecure-content --ignore-certificate-errors
```

```bash
$ cargo test --features=devtools,server,png_widget,test_dependencies -- --nocapture
```

## Testing `webkit2gtk` engine

```bash
$ cargo test --features=webkit,server,png_widget,test_dependencies -- --nocapture
``` 

## License

This project is licensed under the [MIT license](LICENSE).
