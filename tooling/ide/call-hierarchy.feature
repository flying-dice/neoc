Feature: Call hierarchy

  Call hierarchy answers two questions about a function: which functions call it
  (incoming) and which functions it calls (outgoing). Both resolve by name over
  the file's top-level functions.

  Scenario: Incoming calls list the callers
    Given the source:
      """
      fn target() -> number { 1 }
      fn a() -> number { target() }
      fn b() -> number { target() + target() }
      fn c() -> number { 0 }
      """
    When I find the callers of "target"
    Then the call hierarchy lists:
      | function |
      | a        |
      | b        |

  Scenario: Outgoing calls list the callees, once each
    Given the source:
      """
      fn leaf() -> number { 1 }
      fn helper() -> number { 2 }
      fn root() -> number { leaf() + helper() + leaf() }
      """
    When I find the callees of "root"
    Then the call hierarchy lists:
      | function |
      | leaf     |
      | helper   |
