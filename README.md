# cmdprobe

A utility for running arbitrary commands and checking their output


## Running locally

Ensure you have a `cmdprobe.yaml` file in the current directory

```shell
# Start the httpbin for testing
docker-compose up -d

# Run cmdprobe with the local config file
RUST_LOG=cmdprobe=INFO cargo run
```
