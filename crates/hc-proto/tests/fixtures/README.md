# Pinned schema fixture

`hc.openrpc.json` is a copy of `schema/hc.openrpc.json` from
[herdr-core](https://github.com/herdrdev/herdr-core) at commit
`ec8adcdf0da38e539e8702f7c37801647da035f3` (protocol generation 1, version
0.x), licensed under Apache-2.0 (see NOTICE).

Example machine and SSH target names in its descriptions were replaced with
neutral ones (`host-b`, `alice@host-b`); the original names deliberately do
not appear anywhere in this repository. To regenerate, copy the schema file
at the pinned commit and substitute its example host and user names the same
way. `tests/schema.rs` checks metadata and method contracts; review changes
to examples for neutrality.
