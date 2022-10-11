## zcash_script

[![Build Status][actions-badge]][actions-url]
[![Latest Version][version-badge]][version-url]
[![Rust Documentation][docs-badge]][docs-url]

[actions-badge]: https://github.com/ZcashFoundation/zcash_script/workflows/Continuous%20integration/badge.svg
[actions-url]: https://github.com/ZcashFoundation/zcash_script/actions?query=workflow%3A%22Continuous+integration%22
[version-badge]: https://img.shields.io/crates/v/zcash_script.svg
[version-url]: https://crates.io/crates/zcash_script
[docs-badge]: https://img.shields.io/badge/docs-latest-blue.svg
[docs-url]: https://docs.rs/zcash_script

Rust bindings to the ECC's `zcash_script` C++ library.

### Developing

This crate works by manually including the `zcash_script` .h and .cpp files,
using `bindgen` to generate Rust bindings, and compiling everything together
into a single library. Due to the way the `zcash_script` is written we unfortunately need
to include a lot of other stuff e.g. the orchard library.

Note that `zcash_script` (the C++ library/folder inside `zcash`) uses some Rust
FFI functions from `zcash`; and it also links to `librustzcash` which is written in Rust.
Therefore, when updating `zcash_script` (this crate), we need to make sure that shared dependencies
between all of those are the same versions (and are patched to the same revisions, if applicable).
To do that, check for versions in:

- `zcash/Cargo.toml` in the revision pointed to by this crate (also check for patches)
- `librustzcash/Cargo.toml` in the revision pointed to by `zcash` (also check for patches)
- `librustzcash/<crate>/Cargo.toml` in the revision pointed to by `zcash`

### Updating `depend/zcash`

We keep a copy of the zcash source in `depend/zcash` with the help of `git subtree`.
It has one single difference that must be enforced everytime it's updated: the root
`Cargo.toml` must be deleted, since otherwise cargo will ignore the entire folder
when publishing the crate (see https://github.com/rust-lang/cargo/issues/8597).

If you need to update the zcash source, run:

```console
git subtree pull -P depend/zcash https://github.com/zcash/zcash.git <ref> --squash
```

where `<ref>` is a reference to a branch, tag or commit (it should be a tag when preparing
a release, but it will be likely a branch or commit when testing).

The command will likely report a conflict due to the deleted `Cargo.toml` file.
Just run

```console
git rm depend/zcash/Cargo.toml
```

and then commit the updates. Note: after updating zcash, the PR that includes it must *not* be
squashed-and-merged, due to how subtree works. Otherwise you will get errors
when trying to update zcash again.

If that ends up happening, you can always `git rm depend/zcash` and start over
(run the same command as above, but with `add` instead of `pull`);
our usage of `subtree` is to just have a convenient way of pulling copies of `zcash`.
(Unfortunately, switching to submodules is not a good solution due to the need of
deleting the `Cargo.toml`.)


### Publishing New Releases

Releases for `zcash-script` are made with the help of [cargo release](https://github.com/sunng87/cargo-release).

**Checklist:**

* create a new branch batch the release commits into a PR
* update `CHANGELOG.md` to document any major changes since the last release
  https://github.com/rust-lang/cargo/issues/8597)
* open a PR to merge your branch into `master`
* locally run `cargo release -- <level>` where `level` can be `patch`, `minor`, or `major` ([source](https://github.com/sunng87/cargo-release/blob/master/docs/reference.md#bump-level))

**NOTE**: It's important to specify the level when using cargo release because of the way it implements the substitutions. We specify a number of automatic substitutions in `Cargo.toml` but they will only be applied if `cargo release` also handles incrementing the version itself, **do not increment the version by hand and then run `cargo release` or `cargo release -- release`, or it will not correctly update all version references in the codebase.**