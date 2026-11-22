# Validates a sieve verdict record (.github/workflows/sieve-verdict.yml).
# Input: `sieve classify --format json` output. $g[0]: `sieve graph --format json`.
# True only when the record is a sieve.classify.v1 object whose `classes`
# holds a boolean for every class the config declares and `run_all` is a
# boolean; `jq -e` turns false (or a parse error) into a non-zero exit.
(type == "object")
and (.schema == "sieve.classify.v1")
and ((.run_all | type) == "boolean")
and ((.classes | type) == "object")
and (($g[0].classes | keys) - (.classes | keys) | length == 0)
and ([.classes[] | type == "boolean"] | all)
and (($g[0].classes | keys | length) > 0)
