Feature: OpenAI-Compatible API
  As an Open WebUI user
  I want to use the /v1/chat/completions endpoint
  So that I can connect ohAgent as a drop-in replacement.

  Scenario: List available models
    Given the daemon is running
    When I GET /v1/models
    Then the response status is 200
    And the response body is a JSON object with "object": "list"
    And the response body contains 'data' array

  Scenario: Non-streaming chat completion
    Given the daemon is running
    When I POST /v1/chat/completions with:
      """
      {
        "model": "deepseek-chat",
        "messages": [
          {"role": "user", "content": "Say hello in exactly one word."}
        ]
      }
      """
    Then the response status is 200
    And the response body contains "id"
    And the response body contains "choices"
    And the response content is not empty

  Scenario: Streaming chat completion via SSE
    Given the daemon is running
    When I POST /v1/chat/completions with:
      """
      {
        "model": "deepseek-chat",
        "messages": [
          {"role": "user", "content": "Say hi."}
        ],
        "stream": true
      }
      """
    Then the response status is 200
    And the response content type is "text/event-stream"
    And the response body contains "data: "

  Scenario: Invalid model returns error
    Given the daemon is running
    When I POST /v1/chat/completions with:
      """
      {
        "model": "nonexistent-model-xyz",
        "messages": [
          {"role": "user", "content": "Hello"}
        ]
      }
      """
    Then the response status is 400 or 500
