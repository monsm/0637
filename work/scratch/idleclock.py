"""Replicate runtime.rs:888's clock arithmetic against robotd's, for 20 s of
50 Hz ticks, and report how fast each idle clock advances."""
from datetime import timedelta
import time
# robotd: idle_head.tick(tick_start, ...) where tick_start is a real Instant per tick
# core:   idle_now = Instant::now() + tick*20ms, ticked every 20 ms of wall clock
base_wall = 0.0
tick = 0
samples=[]
for k in range(1000):
    wall = k*0.02            # the 50 Hz loop's own wall clock
    tick = k
    idle_now = wall + tick*0.02
    if k in (0, 100, 500, 999):
        samples.append((k, wall, idle_now))
print("tick  wall_s  core_idle_clock_s  ratio_to_wall")
for k,wall,idle in samples:
    print(f"{k:4d}  {wall:6.2f}  {idle:17.2f}  {(idle/wall if wall else 0):.3f}")
print("\nrobotd's idle clock advances 1 s per 1 s of wall time (tick_start is a real Instant).")
print("The core's advances 2 s per 1 s of wall time: idle_now = now + tick*20ms.")
