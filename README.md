## Matrix Link

Matrix Link (`mxlink`) is a Rust library (a higher-level abstraction on top of [matrix-rust-sdk](https://github.com/matrix-org/matrix-rust-sdk)) for building Matrix services (bots, etc.)

It's similar to [headjack](https://github.com/arcuru/headjack), but tries to be less opinionated and more [featureful](#-features).

It finds use in the [🤖 baibot](https://github.com/etkecc/baibot) Matrix bot.


### ✨ Features

- 🎈 Easy to use API for getting started with [matrix-rust-sdk](https://github.com/matrix-org/matrix-rust-sdk). See the [examples](./examples/) directory or [baibot](https://github.com/etkecc/baibot)

- 🔒 Encryption

  - (Optional) At-rest encryption of the session file

  - At-rest encryption of the SQLite data store (performed by matrix-rust-sdk itself)

- 🔄 (Optional) Support for using matrix-rust-sdk's [recovery](https://docs.rs/matrix-sdk/latest/matrix_sdk/encryption/recovery/index.html) module for backing up and restoring encryption keys (in case of session / SQLite store data loss)

- 🖴 [Helpers](./src/helpers/account_data_config) for working with Matrix Account Data on a per-room level or globally

- 🗂 Some convenience functions around Matrix APIs
