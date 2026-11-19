// jsdom lacks the pointer-capture and scroll APIs that Radix primitives (Select) call.
// Each shim is installed only when the environment does not provide it.
if (typeof Element !== 'undefined') {
  const proto = Element.prototype as unknown as Record<string, unknown>
  proto.scrollIntoView ??= function scrollIntoView() {}
  proto.hasPointerCapture ??= function hasPointerCapture() {
    return false
  }
  proto.setPointerCapture ??= function setPointerCapture() {}
  proto.releasePointerCapture ??= function releasePointerCapture() {}
}
