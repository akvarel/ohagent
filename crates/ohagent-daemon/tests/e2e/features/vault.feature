Feature: Vault Integration
  As an operator
  I want to check the Vault connection status
  So that I know secrets are being resolved correctly.

  Scenario: Vault health endpoint reports availability
    Given the daemon is running
    When I GET /api/vault/health
    Then the response status is 200
    And the response body is a JSON object with "available"
    And the response body is a JSON object with "healthy"

  Scenario: Vault status endpoint reports sealed state
    Given the daemon is running
    When I GET /api/vault/status
    Then the response status is 200
    And the response body is a JSON object with "available"
    And the response body is a JSON object with "sealed"
    And the response body is a JSON object with "token_set"

  Scenario: Vault unavailability is handled gracefully
    Given the daemon is running without Vault configured
    When I GET /api/vault/health
    Then the response status is 200
    And the response body contains "available": false
    And the response body contains "healthy": false
