# Provider automatic disablement policy

Providers are automatically disabled for the current local day after three consecutive provider-attributable failures across all routed models and endpoints. The policy counts failed upstream attempts even when a later provider satisfies the user request, records counted failures in a separate provider failure event log for diagnosis, and affects route eligibility only; maintenance actions such as connection tests, balance checks, and pricing sync remain available.
