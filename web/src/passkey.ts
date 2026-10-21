// 조작 기기 passkey(관리 GUI W7): 공개 주소(HTTPS)로 연 조작 기기는 승인 필요 동작마다 이 기기 passkey의 사용자 확인(UV)을
// 한 번 한다(답 1A). 등록은 짝지은 뒤 10분 안에서만 한다(답 2A). host Mac에서 연 loopback 화면은 passkey 없이 승인한다.
// 브라우저 WebAuthn과 host 사이 바이트는 모두 base64url(채움 없음)이다. host가 서명·challenge·화면 주소를 모두 다시 확인한다.
import type { PasskeyAssertion, PasskeyChallenge, PasskeyCreation, PasskeyRegistration, PasskeyState } from "./gen/proto";

export function toB64url(bytes: ArrayBuffer | Uint8Array): string {
  const b = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  let s = "";
  for (const c of b) s += String.fromCharCode(c);
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export function fromB64url(text: string): Uint8Array<ArrayBuffer> {
  const s = atob(text.replace(/-/g, "+").replace(/_/g, "/") + "=".repeat((4 - (text.length % 4)) % 4));
  const out = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i);
  return out;
}

/** 인증기 창을 닫았거나 시간이 지났다(NotAllowedError)를 사람 말로 */
function why(e: unknown): Error {
  if (e instanceof DOMException && e.name === "NotAllowedError") return new Error("passkey 확인을 취소했거나 시간이 지났다.");
  if (e instanceof DOMException && e.name === "InvalidStateError") return new Error("이 인증기에는 이미 이 기기의 passkey가 있다.");
  return e instanceof Error ? e : new Error(String(e));
}

const TIMEOUT_MS = 120_000;

/** `navigator.credentials.create`: ES256·EdDSA, 사용자 확인 필수, attestation 없음 */
export async function createPasskey(c: PasskeyCreation): Promise<PasskeyRegistration> {
  let cred: Credential | null;
  try {
    cred = await navigator.credentials.create({
      publicKey: {
        challenge: fromB64url(c.challenge),
        rp: { id: c.rp_id, name: c.rp_name },
        user: { id: fromB64url(c.user_id), name: c.user_name, displayName: c.user_name },
        pubKeyCredParams: [
          { type: "public-key", alg: -7 },
          { type: "public-key", alg: -8 },
        ],
        authenticatorSelection: { residentKey: "preferred", userVerification: "required" },
        attestation: "none",
        excludeCredentials: c.exclude.map((id) => ({ type: "public-key", id: fromB64url(id) })),
        timeout: TIMEOUT_MS,
      },
    });
  } catch (e) {
    throw why(e);
  }
  if (!(cred instanceof PublicKeyCredential) || !(cred.response instanceof AuthenticatorAttestationResponse))
    throw new Error("인증기가 passkey를 만들지 않았다.");
  return {
    op_id: c.op_id,
    credential_id: toB64url(cred.rawId),
    client_data_json: toB64url(cred.response.clientDataJSON),
    attestation_object: toB64url(cred.response.attestationObject),
  };
}

/** `navigator.credentials.get`: 이 기기 passkey로 host가 묶은 challenge에 서명한다 */
export async function signPasskey(c: PasskeyChallenge): Promise<PasskeyAssertion> {
  let cred: Credential | null;
  try {
    cred = await navigator.credentials.get({
      publicKey: {
        challenge: fromB64url(c.challenge),
        rpId: c.rp_id,
        allowCredentials: c.allow.map((id) => ({ type: "public-key", id: fromB64url(id) })),
        userVerification: "required",
        timeout: TIMEOUT_MS,
      },
    });
  } catch (e) {
    throw why(e);
  }
  if (!(cred instanceof PublicKeyCredential) || !(cred.response instanceof AuthenticatorAssertionResponse))
    throw new Error("인증기가 서명하지 않았다.");
  return {
    op_id: c.op_id,
    credential_id: toB64url(cred.rawId),
    client_data_json: toB64url(cred.response.clientDataJSON),
    authenticator_data: toB64url(cred.response.authenticatorData),
    signature: toB64url(cred.response.signature),
  };
}

/** 지금 등록할 수 있나: 공개 주소의 조작 기기가 아직 passkey가 없고 짝지은 지 10분이 안 됐다 */
export function canRegister(state: PasskeyState | null | undefined, now = Date.now()): boolean {
  return !!state && !state.registered && state.register_until_ms !== null && state.register_until_ms > now;
}

/** 등록 남은 시간(분, 올림) */
export function minutesLeft(state: PasskeyState, now = Date.now()): number {
  return Math.max(0, Math.ceil(((state.register_until_ms ?? now) - now) / 60_000));
}
