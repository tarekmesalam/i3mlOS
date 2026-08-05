#!/usr/bin/env python3
"""The model relay: the host end of i3mlOS's model channel.

The guest speaks a two-line protocol over a virtio-console port and knows
nothing about HTTP, JSON, vendors or keys. That asymmetry is the design:

    guest -> host   ASK <class> <agent> <length>\\n<prompt bytes>
    host  -> guest  OK <tokens>\\n<answer bytes>
                    ERR <reason>

Everything vendor-shaped lives here, on the host, where it can change without
touching the kernel — and where the kernel never holds a key.

With I3ML_MODEL_ENDPOINT and I3ML_MODEL_KEY set, this calls a real
OpenAI-compatible chat endpoint. Without them it answers deterministically,
so the whole path — capability, budget, journal, channel — is exercised in CI
without a network or a secret.

Third-party code is allowed here: this is host tooling, never in the image
(purity charter, standing rule).
"""

import json
import os
import socket
import sys
import urllib.request


def deterministic(prompt: str, model_class: str) -> tuple[str, int]:
    """An answer with no network and no secrets, so the path can be tested."""
    words = [word for word in prompt.split() if word]
    if model_class == "arabic":
        answer = f"ملخّص: {len(words)} كلمة، أهمها «{words[0] if words else '—'}»."
    else:
        answer = f"summary: {len(words)} words, first is '{words[0] if words else '-'}'"
    # A token count in the same ballpark as a real one, so budgets behave.
    return answer, max(1, (len(prompt) + len(answer)) // 4)


def ask_model(prompt: str, model_class: str) -> tuple[str, int]:
    endpoint = os.environ.get("I3ML_MODEL_ENDPOINT")
    key = os.environ.get("I3ML_MODEL_KEY")
    if not endpoint or not key:
        return deterministic(prompt, model_class)

    model = os.environ.get("I3ML_MODEL_NAME", "gpt-4o-mini")
    system = {
        "arabic": "أجب بالعربية الفصحى، بإيجاز شديد.",
        "fast": "Answer in one short sentence.",
        "frontier": "Answer carefully and concisely.",
    }.get(model_class, "Answer concisely.")

    body = json.dumps(
        {
            "model": model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": prompt},
            ],
            "max_tokens": 200,
        }
    ).encode()
    request = urllib.request.Request(
        endpoint,
        data=body,
        headers={"Content-Type": "application/json", "Authorization": f"Bearer {key}"},
    )
    with urllib.request.urlopen(request, timeout=60) as response:
        payload = json.load(response)
    answer = payload["choices"][0]["message"]["content"].strip()
    tokens = payload.get("usage", {}).get("total_tokens", max(1, len(answer) // 4))
    return answer, tokens


def read_exactly(connection: socket.socket, count: int) -> bytes:
    chunks = b""
    while len(chunks) < count:
        chunk = connection.recv(count - len(chunks))
        if not chunk:
            break
        chunks += chunk
    return chunks


def serve(path: str) -> None:
    if os.path.exists(path):
        os.unlink(path)
    listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    listener.bind(path)
    listener.listen(1)
    print(f"relay: listening on {path}", flush=True)

    # One machine at a time, but any number of machines: a test boots the
    # same image repeatedly, and a relay that served exactly one boot would
    # make the second one look like a kernel bug.
    while True:
        connection, _ = listener.accept()
        print("relay: the machine connected", flush=True)
        try:
            converse(connection)
        except (ConnectionResetError, BrokenPipeError):
            print("relay: the machine hung up", flush=True)
        finally:
            connection.close()


def converse(connection: socket.socket) -> None:
    buffer = b""
    while True:
        # The request line, then exactly as many prompt bytes as it declared.
        while b"\n" not in buffer:
            chunk = connection.recv(4096)
            if not chunk:
                print("relay: the machine went away", flush=True)
                return
            buffer += chunk
        line, buffer = buffer.split(b"\n", 1)
        parts = line.decode("utf-8", "replace").split()
        if len(parts) != 4 or parts[0] != "ASK":
            print(f"relay: rejecting {line[:80]!r}", flush=True)
            connection.sendall(b"ERR malformed request\n")
            continue
        _, model_class, agent, length = parts
        length = int(length)
        while len(buffer) < length:
            chunk = connection.recv(4096)
            if not chunk:
                return
            buffer += chunk
        prompt, buffer = buffer[:length].decode("utf-8", "replace"), buffer[length:]

        print(f"relay: agent {agent} asks the {model_class} class: {prompt[:60]!r}", flush=True)
        try:
            answer, tokens = ask_model(prompt, model_class)
        except Exception as error:  # a failed call is an answer too
            print(f"relay: model call failed: {error}", flush=True)
            connection.sendall(b"ERR upstream\n")
            continue
        reply = f"OK {tokens}\n{answer}".encode()
        connection.sendall(reply)
        print(f"relay: answered in {tokens} tokens", flush=True)


if __name__ == "__main__":
    serve(sys.argv[1] if len(sys.argv) > 1 else "/tmp/i3ml-model.sock")
