// Minimal Chrome DevTools Protocol driver for the app's WebView2 (start the app with
// WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222). Node 24, no dependencies.
//   node cdp.mjs text | click TEXT | clickcss CSS | type CSS TEXT | js EXPR | shot FILE.png | wait TEXT [SECONDS]
import { writeFileSync } from 'node:fs'

const PORT = process.env.CDP_PORT ?? '9222'

async function connect() {
  const targets = await (await fetch(`http://127.0.0.1:${PORT}/json/list`)).json()
  const page = targets.find((t) => t.type === 'page')
  if (!page) throw new Error('no page target')
  const ws = new WebSocket(page.webSocketDebuggerUrl)
  await new Promise((resolve, reject) => {
    ws.onopen = resolve
    ws.onerror = reject
  })
  let id = 0
  const pending = new Map()
  ws.onmessage = (event) => {
    const msg = JSON.parse(event.data)
    if (msg.id && pending.has(msg.id)) {
      pending.get(msg.id)(msg)
      pending.delete(msg.id)
    }
  }
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const n = ++id
      pending.set(n, (msg) => (msg.error ? reject(new Error(JSON.stringify(msg.error))) : resolve(msg.result)))
      ws.send(JSON.stringify({ id: n, method, params }))
    })
  return { send, close: () => ws.close() }
}

async function evaluate(cdp, expression) {
  const r = await cdp.send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })
  if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails).slice(0, 800))
  return r.result.value
}

const CLICK = (needle) => `(() => {
  const needle = ${JSON.stringify(needle.toLowerCase())};
  const nodes = [...document.querySelectorAll('button,a,label,[role=button],[role=tab],[role=radio],[role=checkbox],[role=menuitem],[role=option],summary')];
  const visible = (n) => { const r = n.getBoundingClientRect(); return r.width > 0 && r.height > 0 };
  const hit = nodes.find((n) => visible(n) && (n.innerText || n.getAttribute('aria-label') || '').trim().toLowerCase().includes(needle));
  if (!hit) return 'NOT FOUND';
  hit.scrollIntoView({ block: 'center' });
  hit.click();
  return (hit.tagName + ' ' + (hit.innerText || hit.getAttribute('aria-label') || '').trim()).slice(0, 200);
})()`

const TYPE = (css, text) => `(() => {
  const el = document.querySelector(${JSON.stringify(css)});
  if (!el) return 'NOT FOUND';
  const proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, ${JSON.stringify(text)});
  el.dispatchEvent(new Event('input', { bubbles: true }));
  el.dispatchEvent(new Event('change', { bubbles: true }));
  return 'ok ' + el.value;
})()`

const [cmd, a, b] = process.argv.slice(2)
const cdp = await connect()
try {
  if (cmd === 'text') console.log(await evaluate(cdp, 'document.body.innerText'))
  else if (cmd === 'click') console.log(await evaluate(cdp, CLICK(a)))
  else if (cmd === 'clickcss')
    console.log(await evaluate(cdp, `(() => { const e = document.querySelector(${JSON.stringify(a)}); if (!e) return 'NOT FOUND'; e.scrollIntoView({block:'center'}); e.click(); return 'ok' })()`))
  else if (cmd === 'type') console.log(await evaluate(cdp, TYPE(a, b)))
  else if (cmd === 'js') console.log(JSON.stringify(await evaluate(cdp, a), null, 1)?.slice(0, 20000))
  else if (cmd === 'shot') {
    const r = await cdp.send('Page.captureScreenshot', { format: 'png' })
    writeFileSync(a, Buffer.from(r.data, 'base64'))
    console.log(a)
  } else if (cmd === 'wait') {
    const deadline = Date.now() + Number(b ?? 60) * 1000
    for (;;) {
      const text = String(await evaluate(cdp, 'document.body.innerText')).toLowerCase()
      if (text.includes(a.toLowerCase())) { console.log('found'); break }
      if (Date.now() > deadline) { console.log('TIMEOUT'); process.exitCode = 2; break }
      await new Promise((r) => setTimeout(r, 2000))
    }
  } else console.log('usage: text | click TEXT | clickcss CSS | type CSS TEXT | js EXPR | shot FILE | wait TEXT [S]')
} finally {
  cdp.close()
}
