# soroquake-runner

The Soroquake fuzzing engine and GitHub Action, published separately so any
public repository can use them. Source of truth lives in the main Soroquake
repository under `engine/`; this repository is synced from it.

## Use the action

```yaml
# .github/workflows/soroquake.yml
name: soroquake
on:
  pull_request:
  push:
    branches: [main]
jobs:
  shake:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: Nishu0/soroquake-runner@main
        with:
          token: ${{ secrets.SOROQUAKE_TOKEN }}
          api: ${{ vars.SOROQUAKE_API }}
          time: 120
```

## Write a harness

Add a crate (default name `invariants`) with the `soroquake` crate as a dev
dependency and implement `soroquake::Harness`: `setup`, `generate`, `apply`
and `invariants`. Call `soroquake::run(YourHarness)` from a `#[test]`.

```toml
[dev-dependencies]
soroquake = { git = "https://github.com/Nishu0/soroquake-runner", branch = "main" }
```

## Replay lab

Every campaign starts by replaying scripted attacks from well-known bug
classes (share inflation, rounding direction, allowance overspend and more),
then mutating them. Implement `Harness::lab_op` to map the generic
`LabAction`s to calls on your contracts; cases you can't map are skipped.

Settings come from the environment: `SOROQUAKE_TIME`, `SOROQUAKE_SEED`
(replay a campaign), `SOROQUAKE_STEPS`, `SOROQUAKE_TOKEN`, `SOROQUAKE_API`.

See [Nishu0/vault](https://github.com/Nishu0/vault) for a full example.
