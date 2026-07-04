# Codex Helper

Codex Helper manages local routing and provider configuration for Codex-compatible and Claude-compatible upstream services.

## Language

**Provider**:
An upstream service configuration that can receive routed model requests. In this context, Provider includes both OpenAI-compatible providers and Claude-compatible providers.
_Avoid_: Supplier, vendor

**Provider Failure**:
A routed request outcome that is attributable to provider availability or provider configuration and should count toward automatic disabling. Provider Failure includes network errors, timeouts, rate limits, upstream server errors, response read failures, stream interruptions, protocol conversion failures, and authentication or authorization failures.
_Avoid_: Request failure, error

**Consecutive Provider Failures**:
The current uninterrupted sequence of Provider Failures for a Provider across all routed models and endpoints. A successful routed request for that Provider clears the sequence.
_Avoid_: Model failure count, endpoint failure count

**Failure Sequence Reset**:
The end of a Provider's current Consecutive Provider Failures caused by a successful routed request, daily automatic recovery, or manual re-enablement.
_Avoid_: Counter reset

**Provider Day**:
The local calendar date on the machine running Codex Helper, used to decide whether an automatically disabled Provider can recover.
_Avoid_: UTC day, billing day

**Provider Status**:
The single operational state of a Provider: enabled, disabled, or automatically disabled. Provider Status determines whether a Provider can participate in routing.
_Avoid_: Enabled flag, active flag

**Automatic Provider Disablement**:
A Provider Status entered after a Provider reaches the automatic disablement threshold for Consecutive Provider Failures. It prevents the Provider from participating in routing for the current Provider Day unless a user manually re-enables the Provider.
_Avoid_: Temporary disable, cooldown

**Route Eligibility**:
Whether a Provider may be selected as an upstream candidate for routed model requests. Route Eligibility is affected by Provider Status and model support, but not by maintenance actions such as connection tests, balance checks, or pricing sync.
_Avoid_: Availability
