#!/usr/bin/env python3
"""Watch for new transcripts and send to OpenClaw."""

import os
import time
import requests
import threading
import subprocess

TRANSCRIPT_FILE = "/tmp/stomp-claw-transcript.txt"
OPENCLAW_URL = "http://127.0.0.1:18789/v1/chat/completions"
OPENCLAW_TOKEN = "06b21a7fafad855670f81018f3a455edccaf5dedc470fa0b"
LAST_MOD_TIME = 0

def send_to_openclaw(text: str):
    """Send transcript to OpenClaw and speak the response."""
    print(f"📤 Sending to OpenClaw: {text[:50]}...")
    
    headers = {
        "Authorization": f"Bearer {OPENCLAW_TOKEN}",
        "Content-Type": "application/json"
    }
    
    payload = {
        "model": "openclaw:main",
        "messages": [{"role": "user", "content": text}],
        "stream": False
    }
    
    try:
        resp = requests.post(OPENCLAW_URL, headers=headers, json=payload, timeout=60)
        if resp.status_code == 200:
            result = resp.json()
            # Extract response text
            if "choices" in result and len(result["choices"]) > 0:
                reply = result["choices"][0]["message"]["content"]
                print(f"📢 OpenClaw replied: {reply[:100]}...")
                
                # Speak the reply using ~/bin/speak
                subprocess.run(["~/bin/speak", reply], check=True)
            else:
                print(f"⚠️ Unexpected response: {result}")
        else:
            print(f"❌ OpenClaw error: {resp.status_code} - {resp.text}")
    except Exception as e:
        print(f"❌ Failed to send to OpenClaw: {e}")

def check_for_transcript():
    global LAST_MOD_TIME
    try:
        mtime = os.path.getmtime(TRANSCRIPT_FILE)
        if mtime > LAST_MOD_TIME:
            LAST_MOD_TIME = mtime
            with open(TRANSCRIPT_FILE, 'r') as f:
                text = f.read().strip()
            if text:
                # Send in background thread
                threading.Thread(target=send_to_openclaw, args=(text,)).start()
    except FileNotFoundError:
        pass
    except Exception as e:
        print(f"Error: {e}")

print("👀 Watching for transcripts...")
while True:
    check_for_transcript()
    time.sleep(0.5)
