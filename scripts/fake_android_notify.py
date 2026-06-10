#!/usr/bin/env python3
"""
One-shot client: connects to the message-center's /ws as a fake Android,
sends device.hello, then sends a notification.received envelope.
Verifies the message-center persists it to its notifications table.
"""
import asyncio
import base64
import json
import ssl
import sys
import time
import uuid

import websockets

CENTER_WS = "wss://127.0.0.1:8443/ws"
CENTER_REST = "https://127.0.0.1:8443"

OUR_DEVICE_ID = "11111111-2222-3333-4444-555555555555"
CENTER_DEVICE_ID = None  # will be picked up from the message-center's /health


def b64(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


async def main() -> int:
    ssl_ctx = ssl.create_default_context()
    ssl_ctx.check_hostname = False
    ssl_ctx.verify_mode = ssl.CERT_NONE

    async with websockets.connect(CENTER_WS, ssl=ssl_ctx) as ws:
        # 1. device.hello
        hello = {
            "v": 1,
            "id": str(uuid.uuid4()),
            "ts": int(time.time() * 1000),
            "type": "device.hello",
            "device_id": OUR_DEVICE_ID,
            "payload": {
                "name": "Fake Android",
                "device_type": "android",
                "protocol_version": 1,
                "pubkey": b64(b"\x04" + b"\x00" * 64),  # 65 bytes, not used
            },
        }
        await ws.send(json.dumps(hello))
        print(f"sent device.hello: {hello['id']}")

        # Read hello ack (none expected, but consume any pending frames)
        try:
            for _ in range(3):
                msg = await asyncio.wait_for(ws.recv(), timeout=0.5)
                d = json.loads(msg)
                print(f"<- {d.get('type')}: {d.get('id')}")
        except asyncio.TimeoutError:
            pass

        # 2. notification.received
        notif = {
            "v": 1,
            "id": str(uuid.uuid4()),
            "ts": int(time.time() * 1000),
            "type": "notification.received",
            "device_id": OUR_DEVICE_ID,
            "payload": {
                "id": "notif-test-1",
                "package": "com.example.app",
                "app_name": "Example App",
                "title": "Hello from fake client",
                "content": "This notification was sent from a Python script.",
                "posted_at": int(time.time() * 1000),
                "is_sensitive": False,
            },
        }
        await ws.send(json.dumps(notif))
        print(f"sent notification.received: {notif['payload']['title']}")

        # Wait for any ack
        try:
            for _ in range(3):
                msg = await asyncio.wait_for(ws.recv(), timeout=0.5)
                d = json.loads(msg)
                print(f"<- {d.get('type')}: {d.get('id')}")
        except asyncio.TimeoutError:
            pass

        await ws.close()

    # 3. Verify it shows up in REST
    import urllib.request
    req = urllib.request.urlopen(f"{CENTER_REST}/api/v1/notifications?device_id={OUR_DEVICE_ID}&limit=10")
    body = json.loads(req.read().decode())
    print(f"\nGET /api/v1/notifications returned {len(body['notifications'])} row(s):")
    for n in body["notifications"]:
        print(f"  - {n.get('app_name', '?')}: {n.get('title', '?')}")
    return 0 if body["notifications"] else 1


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
