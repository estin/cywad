# CYWAD frontend  

### Running

```bash
trunk serve
```

Rebuilds the app whenever a change is detected and runs a local server to host it.

There's also the `trunk watch` command which does the same thing but without hosting it.

### Release

```bash
trunk build --release
CYWAD_BASE_API_URL="https://cywad.herokuapp.com" trunk build --release
```
