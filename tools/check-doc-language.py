"""Flag substantial non-English prose in RackForge's Markdown documentation."""

from pathlib import Path
import re
import subprocess

from langdetect import DetectorFactory, detect_langs
from langdetect.lang_detect_exception import LangDetectException

DetectorFactory.seed = 0

FENCE = re.compile(r"^(\x60{3,}|~{3,})(.*)$")
BLOCK_START = re.compile(r"^\s*(?:#{1,6}\s|[-+*]\s|\d+[.)]\s|\|)")
INLINE_CODE = re.compile(r"(\x60+).*?\1")
INLINE_LINK = re.compile(r"!?\[([^]]*)\]\([^)]*\)")
MIN_LETTERS = 80
MIN_CONFIDENCE = 0.95


def markdown_blocks(markdown):
    """Yield the first line and text of each prose block."""
    markdown = re.sub(
        r"<!--.*?-->", lambda match: "\n" * match.group().count("\n"), markdown,
        flags=re.DOTALL,
    )
    block = []
    fence = None
    for line_number, line in enumerate([*markdown.splitlines(), ""], start=1):
        marker = FENCE.match(line.lstrip(" >\t"))
        if fence:
            if marker and marker[1].startswith(fence) and not marker[2].strip():
                fence = None
            continue

        if block and (marker or not line.strip() or BLOCK_START.match(line)):
            yield first_line, block
            block = []
        if marker:
            fence = marker[1]
        elif line.strip() and (block or not line.startswith(("    ", "\t"))):
            if not block:
                first_line = line_number
            block.append(line)


def plain_prose(lines):
    if all(line.lstrip().startswith(">") for line in lines):
        lines = [line.lstrip()[1:].lstrip() for line in lines]
        for index, line in enumerate(lines):
            if line.startswith("English translation:") and line.split(":", 1)[1].strip():
                lines = [line.split(":", 1)[1], *lines[index + 1:]]
                break

    prose = " ".join(lines)
    prose = INLINE_CODE.sub(" ", prose)
    prose = INLINE_LINK.sub(r"\1", prose)
    return re.sub(r"https?://\S+|<[^>]+>", " ", prose)


def non_english_blocks(markdown):
    for line_number, lines in markdown_blocks(markdown):
        prose = plain_prose(lines)
        if sum(character.isalpha() for character in prose) < MIN_LETTERS:
            continue
        try:
            languages = detect_langs(prose)
        except LangDetectException:
            continue
        if languages:
            language = languages[0]
            if language.lang != "en" and language.prob >= MIN_CONFIDENCE:
                yield line_number, language.lang


def main():
    root = Path(__file__).resolve().parent.parent
    files = subprocess.check_output(
        ["git", "ls-files", "-z", "--", "*.md", ":!vendor/**"],
        cwd=root,
        encoding="utf-8",
    ).rstrip("\0").split("\0")
    failed = False
    for filename in files:
        markdown = (root / filename).read_text(encoding="utf-8")
        for line, language in non_english_blocks(markdown):
            print(f"{filename}:{line}: prose appears to be in {language}. Write it in English.")
            failed = True
    if not failed:
        print(f"Checked English prose in {len(files)} Markdown files.")
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
