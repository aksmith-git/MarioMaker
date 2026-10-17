import unittest

from smmfmt.course_id import (InvalidCourseCode, smm1_code_to_data_id, smm2_code_to_data_id,
                              smm2_data_id_to_code)


class Smm2CourseIdTests(unittest.TestCase):
    def test_known_code(self):
        # Worked example from the community documentation of the code algorithm.
        self.assertEqual(smm2_code_to_data_id("2J5-3K2-Y9G"), (6635842, False))
        self.assertEqual(smm2_data_id_to_code(6635842), "2J5-3K2-Y9G")

    def test_round_trip_range(self):
        for data_id in list(range(3_000_000, 3_000_500)) + [31, 63, 64, 0xFFFFF, 50_000_000]:
            for maker in (False, True):
                code = smm2_data_id_to_code(data_id, maker)
                self.assertEqual(smm2_code_to_data_id(code), (data_id, maker))

    def test_lowercase_and_no_dashes(self):
        self.assertEqual(smm2_code_to_data_id("2j53k2y9g")[0], 6635842)

    def test_rejects_bad_check_bits(self):
        with self.assertRaises(InvalidCourseCode):
            smm2_code_to_data_id("2J5-3K2-Y9H")

    def test_rejects_vowels(self):
        with self.assertRaises(InvalidCourseCode):
            smm2_code_to_data_id("AAA-AAA-AAA")


class Smm1CourseIdTests(unittest.TestCase):
    def test_parse(self):
        self.assertEqual(smm1_code_to_data_id("1A2B-0000-0012-3456"), 0x00123456)

    def test_reject(self):
        with self.assertRaises(InvalidCourseCode):
            smm1_code_to_data_id("not-a-code")


if __name__ == "__main__":
    unittest.main()
