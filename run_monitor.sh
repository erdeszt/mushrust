#!/bin/sh

DATABASE_URL=sqlite:mushrooms.db cargo run --bin monitor --features="monitor-bin"
