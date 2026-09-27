from entry_gate import GateInput, entry_gate

assert entry_gate(GateInput("WATCH_LOW_CONFIDENCE",440,262,50,True,True,5,2))["state"]=="LATE"
assert entry_gate(GateInput("WATCH",15,15,5,False,False,15,.2))["state"]=="NO_ENTRY"
assert entry_gate(GateInput("WATCH_LOW_CONFIDENCE",10,8,20,True,True,10,1))["state"]=="BUYABLE_NOW"
assert entry_gate(GateInput("WATCH",33,30,20,True,True,10,2))["state"]=="BUYABLE_NOW"
assert entry_gate(GateInput("BEARISH_AVOID",0,0,10,True,True,10,1))["state"]=="NO_ENTRY"
print("PASS")
