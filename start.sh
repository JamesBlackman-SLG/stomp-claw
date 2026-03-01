#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Kill existing stomp_claw
pkill -f "stomp_claw" 2>/dev/null || true
sleep 1

# Generate beep sounds if they don't exist
if [ ! -f /tmp/beep-down.wav ]; then
    echo "Generating beep sounds..."
    python3 << 'PYEOF'
import wave
import math

def make_beep(filename, freq=880, duration=0.1, volume=0.5):
    sample_rate = 44100
    n = int(sample_rate * duration)
    with wave.open(filename, 'w') as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(sample_rate)
        for i in range(n):
            t = i / sample_rate
            envelope = min(1.0, (n - i) / (sample_rate * 0.01)) * min(1.0, i / (sample_rate * 0.01))
            sample = int(volume * envelope * 32767 * math.sin(2 * math.pi * freq * t))
            w.writeframesraw(bytes([sample & 0xFF, (sample >> 8) & 0xFF]))

make_beep("/tmp/beep-down.wav", 880, 0.1)
make_beep("/tmp/beep-up.wav", 880, 0.1)
make_beep("/tmp/beep-up2.wav", 1100, 0.1)
print("Beeps generated")
PYEOF
fi

# Clear old log
rm -f /tmp/stomp-claw.log

# Start stomp_claw
echo "Starting stomp_claw..."
./target/release/stomp_claw &

sleep 2
echo "stomp_claw started. Log at /tmp/stomp-claw.log"
tail -5 /tmp/stomp-claw.log
