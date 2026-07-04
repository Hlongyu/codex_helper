# Provider status is tri-state

Provider status is modeled as one operational state with three values: enabled, disabled, and automatically disabled. This replaces using a manual enabled flag plus separate automatic-disable metadata because those fields can contradict each other and make route eligibility unclear; legacy enabled values are treated as migration input only.
