#!/bin/bash
pkill -f "stomp_claw" 2>/dev/null && echo "stomp_claw stopped" || echo "stomp_claw was not running"
