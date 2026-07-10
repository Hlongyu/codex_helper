# XXSwitch

XXSwitch manages local routing and provider configuration for Codex-compatible and Claude-compatible upstream services.

## Language

**Provider**:
An upstream service configuration that can receive routed model requests. In this context, Provider includes both OpenAI-compatible providers and Claude-compatible providers.
_Avoid_: Supplier, vendor

**Agent Client**:
A known local coding-agent application that XXSwitch supports, such as by configuring model routing or coordinating shared Skills.
_Avoid_: Provider, supplier, vendor

**Pi Coding Agent**:
An Agent Client that XXSwitch can configure for model routing and Skill Exposure by placing shared Skills in Pi's Skill Location.
_Avoid_: Pi provider, Pi supplier

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
The local calendar date on the machine running XXSwitch, used to decide whether an automatically disabled Provider can recover.
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

**Route Model**:
A model name XXSwitch exposes to Agent Clients and can route to an eligible Provider. Route Models are distinct from Provider-specific upstream model names, which may be reached through model mappings.
_Avoid_: Provider model, upstream model

**Skill**:
A reusable capability directory containing `SKILL.md` that an Agent Client can discover and apply while working. A Skill is treated as user-authored working knowledge rather than as a model-routing Provider or runtime plugin, and XXSwitch manages the whole Skill directory rather than only its `SKILL.md`.
_Avoid_: Plugin, provider, prompt

**Skill Identity**:
The stable name that identifies a Skill across Agent Clients. XXSwitch reads it from the Skill's metadata name when present, and falls back to the Skill's directory name only when metadata is missing; XXSwitch does not rename Skill Identities.
_Avoid_: Path, folder name, display name

**Client Skill**:
A Skill that currently lives in an Agent Client's native skill location and is discoverable by that Agent Client.
_Avoid_: Local skill, private skill

**Skill Location**:
A filesystem location where an Agent Client discovers Skills. An Agent Client may have multiple Skill Locations; XXSwitch may suggest defaults for known Agent Clients, but the user can configure them.
_Avoid_: Shared folder, library

**Writable Skill Location**:
A Skill Location that XXSwitch may manage when promoting or exposing Skills. Read-only, built-in, or otherwise unmanaged Skill Locations can be scanned for discovery but are not modified.
_Avoid_: Skill Location

**Managed Skill Location**:
The single Writable Skill Location for a known Agent Client where XXSwitch creates new Skill Exposures. Other Skill Locations for the same Agent Client may still be scanned for discovery, but XXSwitch does not distribute new Shared Skills into them.
_Avoid_: Skill Location, all writable locations

**Shared Skill**:
A canonical Skill managed by XXSwitch and made discoverable to selected Agent Clients without creating independent content copies.
_Avoid_: Copied skill, duplicated skill, public skill

**Skill Sharing Scope**:
The set of Agent Clients that a Shared Skill is intended to be exposed to. Each Shared Skill has its own Skill Sharing Scope chosen by the user; XXSwitch does not inspect Skill content to decide whether an Agent Client is compatible.
_Avoid_: Global sharing, automatic sharing

**Skill Unsharing**:
The removal of a Shared Skill from one Agent Client's Skill Sharing Scope. Skill Unsharing removes that Agent Client's Skill Exposure but does not delete the Shared Skill from the Skill Library.
_Avoid_: Delete, uninstall

**Shared Skill Deletion**:
The removal of a Shared Skill's canonical content from the Skill Library. Shared Skill Deletion includes reviewing the affected Skill Exposures, removing those exposures, and then deleting the canonical Shared Skill.
_Avoid_: Unshare, disable

**Skill Library**:
The XXSwitch-owned collection of Shared Skills. Agent Client skill locations expose Skills from the Skill Library, but do not own the canonical Shared Skill content. Each Skill Identity is unique within the Skill Library.
_Avoid_: Codex skills directory, client skill folder

**Skill Library Root**:
The XXSwitch-managed filesystem location where the Skill Library stores canonical Shared Skill content. The Skill Library Root is not user-configurable.
_Avoid_: Agent Client skill folder, exposure path

**Skill Promotion**:
The act of turning a Client Skill into a Shared Skill by establishing its canonical content in the Skill Library and replacing the original Agent Client location with an exposure of that Shared Skill. Skill Promotion may originate from any Writable Skill Location and must preserve a recoverable path if XXSwitch cannot complete the exposure.
_Avoid_: Copy, import

**Skill Origin**:
The Agent Client, Skill Location, and original path from which a Shared Skill was promoted. Skill Origin is audit history and does not make the originating Agent Client the owner of the Shared Skill.
_Avoid_: Owner, source of truth

**Skill Exposure**:
The relationship that makes a Shared Skill discoverable to an Agent Client through that Agent Client's native discovery mechanism. The exposure may be implemented by links, generated entries, or client-specific adapters, but the exposed Shared Skill remains canonical in the Skill Library.
_Avoid_: Symlink, copy

**Managed Skill Exposure**:
A Skill Exposure created and tracked by XXSwitch. Managed Skill Exposures are shown as exposures of Shared Skills rather than as independent Client Skills during discovery.
_Avoid_: Client Skill, duplicate skill

**Exposure Registry**:
XXSwitch's record of Managed Skill Exposures. The Exposure Registry is the authoritative source for which Agent Client locations are managed exposures, rather than inferring management solely from filesystem paths.
_Avoid_: Path inference, scan result

**Exposure Health**:
The consistency state between the Exposure Registry and the Agent Client-visible filesystem entry for a Managed Skill Exposure. Missing, orphaned, and broken exposures require explicit user repair rather than silent automatic repair.
_Avoid_: Sync status, availability

**Skill Name Conflict**:
A situation where an Agent Client already has a Client Skill with the same identity as a Shared Skill that XXSwitch could expose to it, or where a Client Skill selected for sharing has the same Skill Identity as an existing Shared Skill. XXSwitch does not merge, rename, or overwrite Skills automatically; the user must choose whether to use the existing Shared Skill or keep the Client Skill unshared.
_Avoid_: Merge, overwrite

**Skill Location Conflict**:
A situation where the same Agent Client discovers multiple Client Skills with the same Skill Identity from different Skill Locations. XXSwitch does not merge or choose between them automatically.
_Avoid_: Duplicate skill, priority
