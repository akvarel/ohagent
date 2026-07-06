Feature: Skills API
  As an agent user
  I want to query learned skills
  So that I can see what the agent has learned.

  Scenario: List all active skills
    Given the daemon is running
    When I GET /api/skills?status=active
    Then the response status is 200
    And the response body is a JSON array

  Scenario: List skills with default status filter
    Given the daemon is running
    When I GET /api/skills
    Then the response status is 200
    And the response body is a JSON array

  Scenario: Query skills for a specific tenant
    Given the daemon is running
    When I GET /api/skills?tenant_id=test_e2e
    Then the response status is 200
