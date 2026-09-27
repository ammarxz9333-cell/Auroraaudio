from entry_gate import GateInput, entry_gate

assert entry_gate(GateInput("WATCH_LOW_CONFIDENCE",440,262,50,True,True,5,2))["state"]=="LATE"
assert entry_gate(GateInput("WATCH",15,15,5,False,False,15,.2))["state"]=="NO_ENTRY"
assert entry_gate(GateInput("WATCH_LOW_CONFIDENCE",10,8,20,True,True,10,1))["state"]=="BUYABLE_NOW"
assert entry_gate(GateInput("WATCH",33,30,20,True,True,10,2))["state"]=="BUYABLE_NOW"
assert entry_gate(GateInput("BEARISH_AVOID",0,0,10,True,True,10,1))["state"]=="NO_ENTRY"
print("PASS")


# Regression: a >=100% premarket reprice must never become BUYABLE.
late = entry_gate(GateInput("WATCH", 120, 105, 25, True, True, 10, 0.5))
assert late["state"] == "LATE"

# Regression: ordinary confirmed tape remains BUYABLE when premarket reprice is known and modest.
confirmed = entry_gate(GateInput("WATCH", 8, 7.5, 4, True, True, 12, 0.4))
assert confirmed["state"] == "BUYABLE_NOW"
