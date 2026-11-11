from pathlib import Path
import runpy
import unittest


non_english_blocks = runpy.run_path(
    str(Path(__file__).resolve().parents[1] / "check-doc-language.py")
)["non_english_blocks"]

SPANISH = (
    "Para seleccionar una salida de audio, abra la configuración del sistema "
    "y elija la interfaz que está conectada al instrumento. Después puede "
    "guardar el programa para tocar con el mismo sonido en otra sesión."
)
ENGLISH = (
    "Open the audio settings and select the interface connected to the "
    "instrument. Save the program to play the same sound in another session."
)


class DocumentationLanguageTests(unittest.TestCase):
    def test_reports_the_line_of_a_non_english_paragraph(self):
        markdown = f"# Audio\n\n{SPANISH}\n"
        self.assertEqual(list(non_english_blocks(markdown)), [(3, "es")])

    def test_accepts_a_translated_quotation(self):
        markdown = f"> {SPANISH}\n>\n> English translation: {ENGLISH}\n"
        self.assertEqual(list(non_english_blocks(markdown)), [])

    def test_checks_a_quotation_without_a_translation(self):
        self.assertEqual(list(non_english_blocks(f"> {SPANISH}")), [(1, "es")])

    def test_ignores_code_and_short_product_names(self):
        markdown = (
            f"# Roland JUNO-106 and Érard\n\n```text\n{SPANISH}\n```\n\n"
            f"Use `{SPANISH}` in this example. {ENGLISH}"
        )
        self.assertEqual(list(non_english_blocks(markdown)), [])
