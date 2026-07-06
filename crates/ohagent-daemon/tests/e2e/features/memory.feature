Feature: Memory API
  As an agent user
  I want to search and retrieve memories
  So that the agent can recall past context.

  Scenario: Query memories with search term
    Given the daemon is running
    When I GET /api/memory?q=test
    Then the response status is 200
    And the response body is a JSON array

  Scenario: Query memories without search term returns all
    Given the daemon is running
    When I GET /api/memory
    Then the response status is 200
    And the response body is a JSON array
