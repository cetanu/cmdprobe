# cmdprobe

`cmdprobe` runs shell commands or HTTP requests and checks their output with
exact, regular-expression, JSON-subset, or JMESPath matchers.

## Quick start

The repository includes a complete, runnable [example configuration](cmdprobe.yaml).
It demonstrates multiple YAML documents, retries, captured variables, JSON
matching, saved JMESPath values, and HTTP checks.

```shell
cargo install cmdprobe
cmdprobe --config-file ./cmdprobe.yaml
```

For local development, start the example HTTP service first:

```shell
docker compose up -d
RUST_LOG=cmdprobe=info cargo run -- --config-file ./cmdprobe.yaml
```

Configuration files may contain multiple YAML documents. Each document is one
check, with a `test_name` and an ordered list of `stages`:

```yaml
test_name: curl json check
stages:
  - name: get the JSON document
    check: 'curl http://localhost/json'
    matchers:
      - json:
          slideshow:
            slides:
              - title: "Wake up to WonderWidgets!"
        save:
          author: slideshow.slides[0].title
```

See [`cmdprobe.yaml`](cmdprobe.yaml) for the full example and the available
configuration shapes.

## Usage

The config path defaults to `cmdprobe.yaml` and can also be set with
`CMDPROBE_CONFIG_FILE`.

Each stage supports:

- `check`: a shell command string, or an HTTP request with `url`, `method`, and
  optional `headers`.
- `matchers`: `exact`, `regex`, `json`, or `jmespath` checks. JSON matchers
  check that the expected value is a subset of the response.
- `max_retries`, `delay_before`, and `delay_after`, with delays in seconds.
- `{{ name }}` and `{{ 1 }}` variable references populated by `save` or regex
  capture groups.

Shell commands and configuration are trusted input: shell checks are executed
through `sh -c`.

### StatsD metrics

StatsD output is disabled by default. Supply an address to emit metrics for
the probe, check, and stage results:

```shell
cmdprobe --config-file /etc/cmdprobe.yml --statsd-address 127.0.0.1:8125
```

The following counters are emitted:

```text
cmdprobe.probe.failed / cmdprobe.probe.passed
cmdprobe.check.failed / cmdprobe.check.passed
cmdprobe.stage.failed / cmdprobe.stage.passed
```

## Development

```shell
cargo fmt --all -- --check
cargo check --all-targets --all-features
cargo test --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
```

## Inspiration

I modelled this tool after [Tavern](https://taverntesting.github.io/) so that my
team would have an easy time understanding how to use it and migrate existing tests across.
