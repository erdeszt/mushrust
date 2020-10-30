#!/bin/sh

scp target/arm-unknown-linux-gnueabi/release/mushrust pi@192.168.1.79:/home/pi/dev/mushrust
scp ui/index.html pi@192.168.1.79:/home/pi/dev/ui/index.html
