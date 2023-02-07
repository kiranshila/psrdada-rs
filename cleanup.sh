#!/bin/env bash

max=30 # How many tests do we want to cleanup
for((i=200;i<=max+200;i++))
do
  i=$(printf "%#x\n" $i)
  dada_db -k $i -d
done
