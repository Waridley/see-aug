#! /bin/bash

echo -n PK | dd conv=notrunc bs=1 count=2 of="$1"
