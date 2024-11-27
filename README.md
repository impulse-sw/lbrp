# lbrp

Load balancer & reverse proxy. With QUIC and config hot reload.

> NOTE: for v0.1.0 it's reverse proxy only. No load balancer, no any type of authentication. If you need more flexibility, consider to use `nginx` or `HAProxy`.

## Build

Well, the building process is very easy. You need to install Rust first:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

After installation, clone this repo:

```bash
git clone https://github.com/impulse-sw/lbrp.git
cd lbrp
```

And build the `lbrp`:

```bash
RUSTFLAGS="--cfg reqwest_unstable" cargo build --release
```

### Build with `deployer`

If you have `deployer` and `upx` preinstalled, you can simply build `lbrp` via:

```bash
deployer build
```

## Startup

You have to complete `lbrp-service.yaml` configuration (see [`cc-server-kit` README.md - YAML config example](https://github.com/markcda/cc-server-kit?tab=readme-ov-file#4-quick-start-steps)). After this, load services list into `lbrp-config.json` like this:

```json
{
  "lbrp_mode": "Single",
  "services": [
    {
      "from": "127.0.0.1",
      "to": "http://127.0.0.1:8019"
    },
    {
      "from": "localhost",
      "to": "http://127.0.0.1:8020"
    }
  ]
}
```

> NOTE: don't edit `lbrp_mode` for now (until v0.3.0).

`lbrp` will convert all requests:

- for `127.0.0.1/<something?>` to `http://127.0.0.1:8019/<something?>` and
- for `localhost/<something?>` to `http://127.0.0.1:8020/<something?>`.
