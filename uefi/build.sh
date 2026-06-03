#!/bin/sh
cargo +nightly build --target x86_64-unknown-uefi -Z build-std=std,panic_abort
