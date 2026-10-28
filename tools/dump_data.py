import sys, json
from unittest.mock import MagicMock
for _m in ["spacy","spacy.util","spacy.tokens","spacy.language","phonemizer","phonemizer.backend","phonemizer.backend.espeak","phonemizer.backend.espeak.wrapper","espeakng_loader","curated_tokenizers","spacy_curated_transformers"]:
    sys.modules.setdefault(_m, MagicMock())
import misaki.ja as ja
out = (sys.argv[1] if len(sys.argv) > 1 else "data") + "/"
# HEPBURN: key<TAB>value、1行1組。キーは1〜2文字のかな/記号。
with open(out + "hepburn.tsv", "w", encoding="utf-8", newline="\n") as f:
    for k, v in ja.HEPBURN.items():
        assert "\t" not in k and "\n" not in k and "\t" not in v and "\n" not in v
        f.write(f"{k}\t{v}\n")
print("hepburn", len(ja.HEPBURN))
print("ext", len(ja.Katakana_Phonetic_Extensions), "words", len(ja.JA_WORDS))
