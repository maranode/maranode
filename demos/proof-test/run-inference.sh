#!/usr/bin/env bash
# stand-in for the llama.cpp inference path. deterministic: same prompt gives the
# same bytes, so the reproducible-inference replay works. the real daemon runs
# the model here; the demo returns a fixed answer so no GPU or model download is
# needed. the receipt and verification around it are the real thing.
prompt_file="$1"; out_file="$2"
cat > "$out_file" <<'EOF'
Based only on the attached discharge summary, the three medications are:
1. Atorvastatin 20 mg, once daily at night.
2. Metformin 500 mg, twice daily with meals.
3. Lisinopril 10 mg, once daily.
No other medication is named in the document.
EOF
