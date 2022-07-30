#!/bin/env bash

max=20 # How many tests do we want to cleanup
for((i=1;i<=max;i++))
do
  i=$(printf "%#x\n" $i)
  dada_db -k $i -d
done