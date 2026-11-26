#!/usr/bin/env python3
"""Writes the Slint app's translations from WaveDude Next's string tables.

Usage: tools/next-strings.py   (Next's source tree from $WAVEDUDE_NEXT_DIR)

Each `@tr` text of `ui/*.slint` is looked up among Next's English strings (access key underscores dropped, `{0}`
placeholders read as Slint's `{}`) and its German, French and Dutch strings go to
`lang/<lang>/LC_MESSAGES/waveduderust_slint.po`. Texts Next lacks, or has with differing translations, are listed; they
stay English. A text Next has under several keys with differing translations names its key as the context:
`@tr("SectionRun" => "Run")`. A text with a `&` access key, the Windows menu titles, is translated with each language's
own key, Next's `_` written as `&`. A text Next lacks takes the classic app's translation when the classic app's English
has it, its JSON tables read from the commit `CLASSIC` with git; the entry names its key as `classic:<key>`.
"""

import glob
import json
import os
import re
import subprocess
import sys
import xml.etree.ElementTree as ET

NEXT = os.environ.get("WAVEDUDE_NEXT_DIR")
APP = "frontends/waveduderust_slint"
LANGUAGES = ["de", "fr", "nl"]
# master's last commit holding the classic app's string tables
CLASSIC = "e6ceae4626eef7218e0c132725806928152ce83b"
X = "{http://schemas.microsoft.com/winfx/2006/xaml}"
# `@tr("text"` or `@tr("key" => "text"`: group 1 the key, group 2 the text
TR = re.compile(r'@tr\(\s*(?:"((?:[^"\\]|\\.)*)"\s*=>\s*)?"((?:[^"\\]|\\.)*)"')
ESCAPE = re.compile(r'\\(u\{([0-9a-fA-F]+)\}|.)')


def plain(text):
	"""Next's text without its access key markers (`_x`; `__` is a literal underscore), placeholders as Slint's."""
	return re.sub(r"\{\d+\}", "{}", re.sub(r"_(.)", r"\1", text))


def keyed(text):
	"""Next's text with its access key as a menu's `&` (a literal `&` doubled), placeholders as Slint's."""
	text = re.sub(r"_(.)", lambda m: m.group(1) if m.group(1) == "_" else "&" + m.group(1), text.replace("&", "&&"))
	return re.sub(r"\{\d+\}", "{}", text)


def unkeyed(text):
	"""A `&` title as its plain text."""
	return re.sub(r"&(.)", r"\1", text)


def unescape(literal):
	"""A Slint string literal's text: `\\u{..}`, `\\n`, `\\"`, `\\\\` and `\\{` decoded."""
	return ESCAPE.sub(lambda m: chr(int(m.group(2), 16)) if m.group(2) else {"n": "\n"}.get(m.group(1), m.group(1)), literal)


def table(lang):
	path = os.path.join(NEXT, "WaveDudeNext.WPF/Strings", f"Strings.{lang.upper()}.xaml")
	return {e.get(X + "Key"): e.text or "" for e in ET.parse(path).getroot() if e.get(X + "Key")}


def classic(lang):
	path = f"{CLASSIC}:crates/waveduderust_app/assets/strings/{lang}.json"
	return json.loads(subprocess.run(["git", "show", path], check=True, capture_output=True, text=True).stdout)


def po_string(text):
	for a, b in (("\\", "\\\\"), ('"', '\\"'), ("\n", "\\n"), ("\t", "\\t"), ("\r", "\\r")):
		text = text.replace(a, b)
	return '"' + text + '"'


def main():
	if not NEXT:
		sys.exit("set WAVEDUDE_NEXT_DIR to WaveDude Next's source tree")
	english = table("en")
	tables = {lang: table(lang) for lang in LANGUAGES}
	classic_english = classic("en")
	classic_tables = {lang: classic(lang) for lang in LANGUAGES}
	classic_keys = {}
	for key, text in classic_english.items():
		classic_keys.setdefault(plain(text), []).append(key)
	calls = [m for path in sorted(glob.glob(os.path.join(APP, "ui/*.slint"))) for m in TR.finditer(open(path, encoding="utf-8").read())]
	texts = sorted({(m.group(1) or "", unescape(m.group(2))) for m in calls})
	keys = {}
	for key, text in english.items():
		keys.setdefault(plain(text), []).append(key)
	missing, ambiguous = [], []
	found = {}
	for context, text in texts:
		form = keyed if "&" in text else plain
		# a `&` title takes the key whose access key sits where the title's does
		candidates = [k for k in keys.get(unkeyed(text) if form is keyed else text, []) if form(english[k]) == text]
		if context:
			candidates = [k for k in candidates if k == context]
		if not candidates:
			# the classic app's key whose text is this one and whose translations agree
			olds = [k for k in classic_keys.get(unkeyed(text) if form is keyed else text, []) if form(classic_english[k]) == text]
			variants = {tuple(form(classic_tables[lang].get(k, classic_english[k])) for lang in LANGUAGES) for k in olds}
			if not context and len(variants) == 1:
				found[(context, text)] = ("classic:" + olds[0], variants.pop())
			else:
				missing.append(f"{context} => {text}" if context else text)
			continue
		variants = {tuple(form(tables[lang].get(k, english[k])) for lang in LANGUAGES) for k in candidates}
		if len(variants) > 1:
			ambiguous.append(f"{text} ({', '.join(candidates)})")
		found[(context, text)] = (candidates[0], tuple(form(tables[lang].get(candidates[0], english[candidates[0]])) for lang in LANGUAGES))
	for lang in LANGUAGES:
		folder = os.path.join(APP, "lang", lang, "LC_MESSAGES")
		os.makedirs(folder, exist_ok=True)
		with open(os.path.join(folder, "waveduderust_slint.po"), "w", encoding="utf-8") as po:
			po.write("# Generated by tools/next-strings.py from WaveDude Next's Strings.*.xaml and the classic app's tables (GPL-3.0-or-later).\n")
			po.write('msgid ""\nmsgstr ""\n"Content-Type: text/plain; charset=UTF-8\\n"\n')
			po.write(f'"Language: {lang}\\n"\n')
			for (context, text), (key, translations) in found.items():
				ctxt = f"msgctxt {po_string(context)}\n" if context else ""
				translation = translations[LANGUAGES.index(lang)]
				po.write(f"\n#. {key}\n{ctxt}msgid {po_string(text)}\nmsgstr {po_string(translation)}\n")
	print(f"{len(found)} of {len(texts)} texts translated")
	for text in missing:
		print(f"not in Next: {text}")
	for text in ambiguous:
		print(f"several translations, first taken: {text}")


main()
