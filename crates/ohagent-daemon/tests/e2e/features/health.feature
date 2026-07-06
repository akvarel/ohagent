Feature: Health Check API
  As an operator
  I want to verify that the ohAgent daemon is running
  So that I can monitor its health.

  Scenario: Daemon responds to health check
    Given the daemon is running
    When I GET /health
    Then the response status is 200
    And the response body contains "status": "ok"
    And the response body contains "service": "ohagent"

  Scenario: Daemon reports status with all components
    Given the daemon is running
    When I GET /api/status
    Then the response status is 200
    And the response body contains "service": "ohagent"
    And the response body contains "uptime_seconds"
    And the response body contains "provider"
    And the response body contains "skills_enabled"
    And the response body contains "memory_enabled"
    And the response body contains "vault_available"

  Scenario: Health endpoint is reachable on the configured port
    Given the daemon is running on port 9090
    When I make a TCP connection to port 9090
    Then the connection succeeds
