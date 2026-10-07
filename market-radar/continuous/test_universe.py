import unittest
from continuous.universe import parse_directory


class DirectoryTest(unittest.TestCase):
    def test_short_ordinary_word_is_not_company_alias(self):
        self.assertEqual(parse_directory('Symbol|Security Name|Test Issue|ETF\nBLSH|Bullish Ordinary Shares|N|N\n'), {'BLSH':['Bullish Ordinary Shares']})

    def test_excludes_etfs_test_rows_and_footer(self):
        text = 'Symbol|Security Name|Test Issue|ETF\nPENG|Penguin Solutions Inc. - Common Stock|N|N\nFUND|Fund ETF|N|Y\nTEST|Test Issuer|Y|N\nFile Creation Time: today|||\n'
        self.assertEqual(parse_directory(text), {'PENG':['Penguin Solutions Inc. - Common Stock','Penguin Solutions Inc.']})
