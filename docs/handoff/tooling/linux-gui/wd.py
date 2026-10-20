#!/usr/bin/env python3
"""Minimal W3C WebDriver client for tauri-driver (stdlib only).

Usage (inside the GUI container):
  wd.py new [app]            start a session (default /usr/bin/vjumpkung-comfyui-desktop)
  wd.py shot NAME            WebDriver screenshot -> /out/shots/NAME.png
  wd.py screen NAME          full X screen (import -window root) -> /out/shots/NAME.png
  wd.py text                 print document.body.innerText
  wd.py click TEXT           click the first visible button/link/label/role=button whose text contains TEXT
  wd.py clickcss CSS         click the first element matching CSS
  wd.py type CSS TEXT        clear + type into the element matching CSS (React-friendly native setter)
  wd.py js SCRIPT            execute script, print the JSON result
  wd.py wait TEXT [SECONDS]  wait until the page text contains TEXT
  wd.py quit                 delete the session
"""

import base64
import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request

BASE = os.environ.get("WD_URL", "http://127.0.0.1:4444")
SESSION_FILE = "/work/session"
SHOTS = "/out/shots"


def call(method, path, body=None, timeout=120):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(
        BASE + path,
        data=data,
        method=method,
        headers={"Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as res:
            return json.loads(res.read() or b"{}").get("value")
    except urllib.error.HTTPError as err:
        payload = err.read().decode(errors="replace")
        raise SystemExit(f"webdriver {method} {path} -> {err.code}: {payload[:800]}")


def sid():
    with open(SESSION_FILE) as fh:
        return fh.read().strip()


def js(script, *args):
    return call(
        "POST", f"/session/{sid()}/execute/sync", {"script": script, "args": list(args)}
    )


CLICK_JS = r"""
const needle = arguments[0].toLowerCase();
const nodes = [...document.querySelectorAll('button,a,label,[role=button],[role=tab],[role=radio],[role=checkbox],[role=menuitem],[role=option],summary')];
const visible = n => { const r = n.getBoundingClientRect(); return r.width > 0 && r.height > 0; };
const hit = nodes.find(n => visible(n) && (n.innerText || n.getAttribute('aria-label') || '').trim().toLowerCase().includes(needle));
if (!hit) return 'NOT FOUND';
hit.scrollIntoView({block: 'center'});
hit.click();
return (hit.tagName + ' ' + (hit.innerText || hit.getAttribute('aria-label') || '').trim()).slice(0, 200);
"""

TYPE_JS = r"""
const el = document.querySelector(arguments[0]);
if (!el) return 'NOT FOUND';
const proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, arguments[1]);
el.dispatchEvent(new Event('input', {bubbles: true}));
el.dispatchEvent(new Event('change', {bubbles: true}));
return 'ok ' + el.value;
"""


def main(argv):
    cmd = argv[0]
    if cmd == "new":
        app = argv[1] if len(argv) > 1 else "/usr/bin/vjumpkung-comfyui-desktop"
        caps = {
            "capabilities": {"alwaysMatch": {"tauri:options": {"application": app}}}
        }
        value = call("POST", "/session", caps, timeout=180)
        with open(SESSION_FILE, "w") as fh:
            fh.write(value["sessionId"])
        print("session", value["sessionId"])
    elif cmd == "shot":
        os.makedirs(SHOTS, exist_ok=True)
        png = base64.b64decode(call("GET", f"/session/{sid()}/screenshot"))
        path = f"{SHOTS}/{argv[1]}.png"
        with open(path, "wb") as fh:
            fh.write(png)
        print(path)
    elif cmd == "screen":
        os.makedirs(SHOTS, exist_ok=True)
        path = f"{SHOTS}/{argv[1]}.png"
        subprocess.run(["import", "-window", "root", path], check=True)
        print(path)
    elif cmd == "text":
        print(js("return document.body.innerText"))
    elif cmd == "click":
        print(js(CLICK_JS, argv[1]))
    elif cmd == "clickcss":
        print(
            js(
                "const e=document.querySelector(arguments[0]); if(!e) return 'NOT FOUND'; e.scrollIntoView({block:'center'}); e.click(); return 'ok';",
                argv[1],
            )
        )
    elif cmd == "type":
        print(js(TYPE_JS, argv[1], argv[2]))
    elif cmd == "js":
        print(json.dumps(js(argv[1]), ensure_ascii=False)[:20000])
    elif cmd == "wait":
        deadline = time.time() + (float(argv[2]) if len(argv) > 2 else 60)
        while time.time() < deadline:
            text = js("return document.body.innerText") or ""
            if argv[1].lower() in text.lower():
                print("found")
                return
            time.sleep(2)
        raise SystemExit("timeout waiting for " + argv[1])
    elif cmd == "quit":
        call("DELETE", f"/session/{sid()}")
        print("closed")
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main(sys.argv[1:] or ["help"])
