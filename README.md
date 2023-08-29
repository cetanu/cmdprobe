# cmdprobe
A utility for running arbitrary commands and checking their output


## Install
```shell
cargo install cmdprobe
```


## Usage

### Basic execution
See the example `cmdprobe.yaml` file for what configuration is available.

Construct your own configuration file, and then run cmdprobe against it to execute
all the checks that you need to do.

```shell
cmdprobe --config-file /etc/cmdprobe.yml
```

### Emitting statsd metrics
You can supply a statsd host and `cmdprobe` will emit metrics for each test & stage.

```shell
cmdprobe --config-file /etc/cmdprobe.yml --statsd-address 127.0.0.1:8125
```

The following metrics will be emitted:

```
# Did the entire probe run fail/succeed
cmdprobe.probe.failed
cmdprobe.probe.passed

# Did one check (a collection of stages) fail/succeed
cmdprobe.check.failed
cmdprobe.check.passed

# Did an indidivual stage within a check fail/succeed
cmdprobe.stage.failed
cmdprobe.stage.passed
```


## Running locally
Ensure you have a `cmdprobe.yaml` file in the current directory

```shell
# Start the httpbin for testing
docker-compose up -d

# Run cmdprobe with the local config file
RUST_LOG=cmdprobe=INFO cargo run
```

## Inspiration

I modelled this tool after [Tavern](https://taverntesting.github.io/) so that my
team would have an easy time understanding how to use it and migrate existing tests across.
