# Contributing to tableski

Thanks for helping. Small, focused pull requests are the easiest to review; open an issue first
for new formats, new tools, or anything that changes CLI flag behaviour (the flags are a public
contract).

## Before you open a pull request

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test            # includes the HTTP smoke tests
cargo deny check      # licences + advisories (config in deny.toml)
```

New ingest paths come with a fixture in `fixtures/corpus/` and a test in
`tests/excel_hardening.rs` or `tests/formats_export.rs`. Keep `README.md` and `ROADMAP.md` in
step with the code.

## Developer Certificate of Origin

This project uses the [Developer Certificate of Origin](https://developercertificate.org/)
(DCO) instead of a contributor licence agreement. By adding a sign-off line to your commits
you certify that you wrote the change or otherwise have the right to submit it under the
project licence.

Sign off every commit:

```bash
git commit -s
```

which adds a line such as

```
Signed-off-by: Your Name <you@example.com>
```

Use a real name and a reachable email address.

## Licence

tableski is dual licensed under MIT OR Apache-2.0 (see [LICENSE-MIT](./LICENSE-MIT) and
[LICENSE-APACHE](./LICENSE-APACHE)). Unless you explicitly state otherwise, any contribution
you submit is licensed the same way, without any additional terms or conditions.
