import unittest
from continuous.classification import classify
from continuous.core import utc


class ClassificationTests(unittest.TestCase):
    def test_public_snapshot_cannot_authorize_early_entry(self):
        result=classify([{'confidence':95,'official':True}],{'price':65,'change_pct':2},None,1000)
        self.assertFalse(result['buyable'])
        self.assertEqual(result['classification'],'DEVELOPING')

    def test_peng_73_74_is_late_relative_to_64_66(self):
        for anchor in (64,66):
            for price in (73,74):
                result=classify([],{'since_event_move_pct':(price/anchor-1)*100},None,1000)
                self.assertEqual(result['classification'],'LATE')
                self.assertFalse(result['buyable'])

    def test_risk_overrides_late_and_positive_tape(self):
        alert=dict(classification='CONFIRMED',buyable=True,detected_at=utc(990),tape_stale=False,trigger=64,invalidation=63)
        result=classify([{'dilution':True}],{'change_pct':10},alert,1000)
        self.assertEqual(result['classification'],'AVOID')

    def test_stale_signal_cannot_authorize_buy(self):
        alert=dict(classification='HIGH-CONVICTION EARLY',buyable=True,detected_at=utc(800),tape_stale=False,trigger=64,invalidation=63)
        self.assertFalse(classify([],{},alert,1000)['buyable'])
        alert['detected_at']=utc(990)
        self.assertTrue(classify([],{},alert,1000)['buyable'])

    def test_day_move_warning_does_not_claim_source_anchor(self):
        result=classify([],{'change_pct':12},None,1000)
        self.assertEqual(result['classification'],'LATE')
        self.assertIn('الإغلاق السابق',result['reasons'][0])

    def test_old_avoid_alert_is_not_current_risk_evidence(self):
        result=classify([],{},dict(classification='AVOID',detected_at=utc(1),tape_stale=True),1000)
        self.assertEqual(result['classification'],'DEVELOPING')

if __name__ == '__main__':
    unittest.main()
