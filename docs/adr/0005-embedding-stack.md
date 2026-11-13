# ADR 0005: Local embeddings for the optional vector extra

- Status: Accepted
- Date: 2026-11-13
- Decision references: D11, SPEC section 11

## Context

Vector search is an optional extra, `lucubra[vector]`, off by default. SPEC section 11 asked for the licences of the embedding library and model before a choice.

## Evidence

Read on 2026-11-12 and 2026-11-13 from PyPI and the Hugging Face Hub API:

| Component | Licence | Notes |
|---|---|---|
| `model2vec` 0.10.0 (PyPI) | MIT | Runtime dependencies: jinja2, joblib, numpy, safetensors, tokenizers, tqdm, huggingface-hub. No PyTorch, no ONNX Runtime |
| `minishlab/potion-base-8M` | MIT | About 30 MB of weights; no language tags |
| `minishlab/potion-multilingual-128M` | MIT | About 512 MB of weights; tagged for hi, bn, ta, te, kn, ml, mr and gu among others |
| `fastembed` 0.9.1 (PyPI) with `onnxruntime` 1.31.1 | Apache-2.0 and MIT | Alternative runtime |
| `intfloat/multilingual-e5-small` | MIT | About 118 MB as quantised ONNX |
| `sentence-transformers/all-MiniLM-L6-v2` | Apache-2.0 | Trained in part on MS MARCO, whose terms restrict use to non-commercial research |

## Decision

1. The `vector` extra installs `model2vec` and runs embeddings in a Python worker. A Python extra cannot add Rust code to the wheel, so the Rust core stays free of embedding dependencies and the default install is unchanged.
2. The model is chosen per workspace when vector search is enabled. `potion-base-8M` is used when every subject's explanation language is English and the indexed text is in Latin script. `potion-multilingual-128M` is used otherwise. The user can override the choice in settings.
3. Models are downloaded only when the user enables vector search, and the download size is shown before it starts.
4. `all-MiniLM-L6-v2` and other models trained on MS MARCO are not offered.

## Consequences

- The multilingual model is a large download. The planner and search work fully without it.
- The licences of indirect dependencies of `model2vec` (jinja2, joblib and others) are checked by `scripts/check_python_licenses.py` when the extra is added in M2.
- Retrieval quality of the chosen models on study material is not measured in M0. M10 measures it on the fixture corpus.
