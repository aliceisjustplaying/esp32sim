// Runs the YAML scenarios in ../scenarios (wait-serial, delay, set-control,
// write-serial, take-screenshot) against the VirtualBoard. Extra control for the knob:
//   set-control: { part-id: knob, control: rotate, value: 3 }    (negative = CCW)
//   set-control: { part-id: knob, control: pressed, value: 1 }
#pragma once
#include <string>
struct ScenarioResult { bool ok; std::string message; int steps; };
ScenarioResult runScenario(const std::string& path, int stepTimeoutMs, const std::string& screenshotDir);
