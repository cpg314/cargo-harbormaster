# cargo-harbormaster

Export diagnostics from `cargo check`, `cargo clippy` and [`cargo nextest`](https://nexte.st/) into the JSON message format expected by [Phabricator's Harbormaster](https://secure.phabricator.com/book/phabricator/article/harbormaster/).

This allows reporting errors and test results directly in Phabricator differentials.

See the [Harbormaster API documentation](https://secure.phabricator.com/conduit/method/harbormaster.sendmessage/). Note that the message generated by `cargo-harbormaster` uses the parameters format used by `arc` (see the example below; the parameters and API tokens are encoded in a single JSON message).

For `cargo nextest`, we rely on a regular expression to parse the output, as machine-readable output is [not supported yet](https://nexte.st/book/machine-readable.html#running-tests).

## Usage

```console
$ cargo clippy --message-format=json > clippy.json
$ cargo nextest 2 > nextest.log

$ export PHAB_TOKEN=...
$ params=$(cargo-harbormaster {PHID-...} --status pass --clippy-json clippy.json --nextest-stderr nextest.log)

$ curl -X POST https://{...}/api/harbormaster.sendmessage -d params="$params
```

## Command line arguments

```
Usage: cargo-harbormaster [OPTIONS] --token <TOKEN> --status <STATUS> <BUILD_PHID>

Arguments:
  <BUILD_PHID>  Build PHID (PHID-...)

Options:
      --workspace <WORKSPACE>
          Path to the rust workspace relative to the repository root
      --token <TOKEN>
          Phabricator API token [env: PHAB_TOKEN=]
      --status <STATUS>
          Build status [possible values: abort, fail, pass, pause, restart, resume, work]
      --clippy-json <CLIPPY_JSON>
          Path to 'cargo clippy --message-format=json' output
      --check-json <CHECK_JSON>
          Path to 'cargo check --message-format=json' output
      --nextest-stderr <NEXTEST_STDERR>
          Path to 'cargo nextest' stderr output
  -h, --help
          Print help
```