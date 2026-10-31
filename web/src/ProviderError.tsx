import type { AgentStatus } from "./gen/proto";
import { providerFailure } from "./providerFailure";
import "./ProviderError.css";

export function ProviderError({ agent }: { agent: AgentStatus | null | undefined }) {
  const failure = providerFailure(agent);
  if (!failure) return null;
  return (
    <div className="banner err provider-error" role="status" data-testid="provider-error" data-kind={failure.kind}>
      <strong>{failure.reason}</strong>{" · "}<span>{failure.evidence}</span>
    </div>
  );
}
