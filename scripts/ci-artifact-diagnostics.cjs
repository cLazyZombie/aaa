const { appendFileSync } = require("node:fs");
const { subscribe } = require("node:diagnostics_channel");
const { request } = require("node:https");

function publicHost(value) {
  // 공개 로그에는 GitHub Actions와 Azure의 공개 DNS 이름만 남긴다.
  const host = typeof value === "string" ? value.toLowerCase() : "";
  return /^(?:(?:api\.)?github\.com|(?:[a-z0-9-]+\.)+(?:actions\.githubusercontent\.com|blob\.core\.windows\.net))$/.test(host)
    ? host
    : null;
}

const networkCodes = new Set([
  "ENOTFOUND", "EAI_AGAIN", "EAI_FAIL", "EAI_NONAME", "ENODATA",
  "ECONNREFUSED", "ECONNRESET", "ETIMEDOUT", "EHOSTUNREACH", "ENETUNREACH",
  "ECONNABORTED", "EADDRNOTAVAIL", "EPIPE", "ABORT_ERR",
  "ERR_TLS_CERT_ALTNAME_INVALID", "CERT_HAS_EXPIRED", "UNABLE_TO_VERIFY_LEAF_SIGNATURE",
]);

function errorFields(error) {
  const code = networkCodes.has(error?.code) ? error.code : "기타";
  const syscall = ["getaddrinfo", "getnameinfo", "connect", "read", "write"].includes(error?.syscall)
    ? error.syscall
    : "-";
  const errno = Number.isInteger(error?.errno) ? error.errno : "-";
  return `code=${code} syscall=${syscall} errno=${errno}`;
}

function observeUpload() {
  let resultsHost;
  try {
    resultsHost = publicHost(new URL(process.env.ACTIONS_RESULTS_URL).hostname);
  } catch {
    // 런타임 URL이 없거나 잘못된 경우 실제 업로드 액션이 오류를 판정한다.
  }
  if (resultsHost && process.env.GITHUB_OUTPUT) {
    try {
      appendFileSync(process.env.GITHUB_OUTPUT, `results-host=${resultsHost}\n`);
    } catch {
      console.log("[업로드 진단] 결과 서비스 호스트를 다음 단계에 전달하지 못했다.");
    }
  }

  // 고정한 upload-artifact v7의 Node HTTP 요청에서, SDK가 오류를 다시 포장하기 전에 읽는다.
  subscribe("http.client.request.error", ({ request, error }) => {
    const path = request.path.split("?")[0];
    const rpc = /^\/twirp\/github\.actions\.results\.api\.v1\.ArtifactService\/(CreateArtifact|FinalizeArtifact)$/.exec(path);
    const isTunnel = request.method === "CONNECT";
    const operation = rpc?.[1] ?? (isTunnel ? "CONNECT" : "HTTP");
    const target = publicHost(isTunnel ? path.split(":")[0] : request.host) ?? "비공개";
    const dnsHost = error?.hostname === undefined ? "-" : publicHost(error.hostname) ?? "비공개";
    console.log(`[업로드 요청 오류] operation=${operation} target=${target} dnsHost=${dnsHost} ${errorFields(error)}`);
  });
}

function probe(host) {
  const families = new Set();
  let dnsError;
  let tcp = false;
  let tls = false;
  const req = request({
    hostname: host, method: "HEAD", agent: false, signal: AbortSignal.timeout(5000),
  });
  req.once("socket", (socket) => {
    socket.on("lookup", (error, _address, family) => {
      if (error) dnsError = error;
      else families.add(family === 4 ? "IPv4" : "IPv6");
    });
    socket.once("connect", () => { tcp = true; });
    socket.once("secureConnect", () => { tls = true; });
  });
  function record(http, error) {
    const dns = dnsError ? errorFields(dnsError) : [...families].join(",") || "미관측";
    const failure = error && error !== dnsError ? ` ${errorFields(error)}` : "";
    console.log(`[러너 네트워크] host=${host} dns=(${dns}) tcp=${tcp} tls=${tls} http=${http}${failure}`);
  }
  req.once("response", (response) => {
    response.resume();
    record(response.statusCode);
  });
  req.once("error", (error) => record("-", error));
  req.end();
}

if (require.main === module) {
  const { RUNNER_OS, RUNNER_ARCH, ImageOS, ImageVersion, E2E_SHARD, RESULTS_HOST } = process.env;
  const proxy = ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"].some((name) => !!process.env[name]);
  console.log(`[러너] shard=${E2E_SHARD} os=${RUNNER_OS} arch=${RUNNER_ARCH} image=${ImageOS}/${ImageVersion} proxyConfigured=${proxy}`);
  const host = publicHost(RESULTS_HOST);
  if (!host) console.log("[러너 네트워크] 공개 결과 서비스 호스트를 확인할 수 없다.");
  // 직접 연결로 관측한다. 프록시 설정 유무는 위에 별도로 기록한다.
  for (const target of new Set([host, "github.com"])) {
    if (target) probe(target);
  }
} else {
  observeUpload();
}
