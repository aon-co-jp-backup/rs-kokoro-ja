import sys, json
from unittest.mock import MagicMock
for _m in ["spacy","spacy.util","spacy.tokens","spacy.language","phonemizer","phonemizer.backend","phonemizer.backend.espeak","phonemizer.backend.espeak.wrapper","espeakng_loader","curated_tokenizers","spacy_curated_transformers"]:
    sys.modules.setdefault(_m, MagicMock())
import misaki.ja as ja
g = ja.JAG2P()
rows = []
for line in open("tests/corpus.txt", encoding="utf-8"):
    line = line.rstrip("\n")
    if not line: continue
    r = g(line)
    ph = r[0] if isinstance(r, tuple) else r
    rows.append((line, ph))
with open("tests/expected_phonemes.tsv", "w", encoding="utf-8", newline="\n") as f:
    for a, b in rows:
        f.write(f"{a}\t{b}\n")
print(len(rows))
