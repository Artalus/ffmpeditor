# structure

structure code top down.
first "main" function, then functions called from it, then functions called from there.
no need to comment this structure.
good example:
```
def main():
  foo()
  bar()
  baz()

def foo():
  top()
  kek()

def bar():
 cheburek()

def baz()

def top()

def kek()

def cheburek()
```

# comments

reading code should be enough to understand what it does.
comments should explain *why* not *what*; do not comment if the code is just following the spec.
keep it short; nobody likes to read paragraphs of ai slop.
```
# short comments without capital and period

# Multiline comments start with capital and end with period.
# One sentence per line.
```

# code bloat

strive to write as few code as possible to solve task.
every once in a while we should take sessions to see if we can compact the codebase.
when architect reviews worker code, take extra care to check if simpler solution to problem exists.
