// E12 route가 보류할 마지막 응답과 새 읽기의 첫 요청을 고른다.
export class ThreadReloadGate {
  armed = false;
  held = false;
  document = null;
  previousDocument = null;
  arm(previousDocument) { this.armed = true; this.previousDocument = previousDocument; }
  holdFinal(document, body) {
    if (!this.armed || !document || document === this.previousDocument || this.held || body?.kind !== "threads" || body.next_after) return false;
    this.held = true;
    this.document = document;
    return true;
  }
  holdFirst(document, after) { return this.held && document === this.document && !after; }
}
