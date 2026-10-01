#!/bin/bash

export GPU_MAX_HW_QUEUES=1
export HIP_FORCE_DEV_KERNARG=1

exec /home/eric/Echo_Adapt_v5/adapt_server/target/release/adapt_server \
    --config /home/eric/Echo_Adapt_v5/adapt_server/config.toml
