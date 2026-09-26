# Rendered by Statecraft from profile github-actions-rust revision 7,
# because this repository had no Makefile. Every target calls the same script
# CI runs, so the local and remote gates are one definition.
.PHONY: tools gate code

tools:
	sh scripts/statecraft/install-spec-spine.sh

gate:
	sh scripts/statecraft/gate.sh governance

code:
	sh scripts/statecraft/gate.sh code
