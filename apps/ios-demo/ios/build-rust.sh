#!/bin/bash
set -euo pipefail

echo "Cranpose does not currently ship an iOS backend."
echo "The reserved ios feature must not alias desktop. A real backend needs UIKit,"
echo "CAMetalLayer, CADisplayLink, lifecycle, input, safe-area, and density support."
exit 1
