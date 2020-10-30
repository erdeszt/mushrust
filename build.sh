#!/bin/sh

DATABASE_URL=sqlite:mushrooms.db cargo build --target=arm-unknown-linux-gnueabi --release
