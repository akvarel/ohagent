//! Security guard — prevents the agent from modifying system configuration.
//!
//! ## Threat model
//!
//! The ohAgent pod has a Jcode engine that can execute shell commands,
//! write files, and call APIs. We must prevent it from modifying:
//!
//! 1. K8s resources (ConfigMaps, Secrets, Deployments) — already blocked by RBAC
//! 2. Environment variables within the pod
//! 3. Configuration files (keys.toml, .ohagent/*)
//! 4. System services (systemctl, service)
//!
//! ## Guarantees
//!
//! These checks run BEFORE the command reaches bash.
//! They cannot be bypassed — the daemon owns the tool execution path.

/// Check if a command or file operation is attempting to modify
/// system configuration. Returns `Ok(())` if safe, `Err(reason)` if blocked.
pub fn check_command_safety(command: &str, working_dir: &str) -> Result<(), String> {
    let lower = command.to_lowercase();

    // ── Block kubectl mutations ──
    let kubectl_danger = [
        "kubectl apply", "kubectl create", "kubectl delete", "kubectl patch",
        "kubectl edit", "kubectl replace", "kubectl scale", "kubectl rollout",
        "kubectl annotate", "kubectl label",
        "kubectl set env", "kubectl set image", "kubectl set resources",
    ];
    if lower.contains("kubectl") {
        for pattern in &kubectl_danger {
            if lower.contains(pattern) {
                return Err(format!(
                    "BLOCKED: kubectl mutation command not allowed ({})",
                    pattern
                ));
            }
        }
        // Allow read-only kubectl: get, describe, logs, top, auth can-i
        let allowed = ["kubectl get", "kubectl describe", "kubectl logs",
                       "kubectl top", "kubectl auth can-i", "kubectl api-resources",
                       "kubectl explain", "kubectl version"];
        if !allowed.iter().any(|a| lower.contains(a)) {
            return Err(
                "BLOCKED: kubectl commands are restricted to read-only operations".into()
            );
        }
    }

    // ── Block systemd / service mutations ──
    for sys_cmd in &["systemctl ", "service ", "systemd-run ", "initctl "] {
        if lower.contains(sys_cmd) && is_mutation(&lower, sys_cmd) {
            return Err(format!(
                "BLOCKED: system service modification not allowed ({})",
                sys_cmd.trim()
            ));
        }
    }

    // ── Block env var mutation through export/set/setenv ──
    let protected_envs = [
        "DEEPSEEK_API_KEY", "ANTHROPIC_API_KEY", "OPENAI_API_KEY",
        "TELEGRAM_BOT_TOKEN", "GOOGLE_API_KEY", "GEMINI_API_KEY",
        "SF_API_KEY", "ZAI_API_KEY", "SCW_SECRET_KEY", "SCW_PROJECT_ID",
        "GROQ_API_KEY", "VAULT_TOKEN", "VAULT_ADDR", "OHAGENT_ADMIN_USER_ID",
        "KUBECONFIG",
    ];
    if lower.contains("export ") || lower.contains("setenv ") || lower.starts_with("set ") {
        for env in &protected_envs {
            if lower.contains(&env.to_lowercase()) {
                return Err(format!(
                    "BLOCKED: cannot modify protected environment variable {}",
                    env
                ));
            }
        }
    }

    // ── Block writing to protected paths ──
    let protected_paths = [
        "/home/jcode/.ohagent/keys.toml",
        "/home/jcode/.ohagent",
        "/etc/kubernetes",
        "/var/run/secrets",
        "/proc/1/environ",
        "/vault",
        "/usr/local/bin/ohagent-daemon",
    ];
    for path in &protected_paths {
        if lower.contains(path) && (lower.contains(">") || lower.contains("write") || lower.contains("rm ") || lower.contains("mv ") || lower.contains("cp ")) {
            return Err(format!(
                "BLOCKED: cannot modify protected path {}",
                path
            ));
        }
    }

    // ── Block helm mutations ──
    let helm_danger = ["helm install", "helm upgrade", "helm uninstall", "helm delete",
                       "helm rollback", "helm template"];
    if lower.contains("helm ") {
        for pattern in &helm_danger {
            if lower.contains(pattern) {
                return Err(format!(
                    "BLOCKED: helm mutation command not allowed ({})",
                    pattern
                ));
            }
        }
    }

    // ── Block docker mutations ──
    let docker_danger = ["docker run", "docker build", "docker push", "docker rm",
                         "docker rmi", "docker tag", "docker exec", "docker stop",
                         "docker kill", "docker compose"];
    if lower.contains("docker ") && !lower.contains("docker ps") && !lower.contains("docker images") && !lower.contains("docker logs") {
        for pattern in &docker_danger {
            if lower.contains(pattern) {
                return Err(format!(
                    "BLOCKED: docker mutation command not allowed ({})",
                    pattern
                ));
            }
        }
    }

    // ── Block K8s auth file modification ──
    if lower.contains("kubeconfig") || lower.contains(".kube/config")
        || lower.contains("~/.kube") {
        if lower.contains("write") || lower.contains("echo ") || lower.contains(">")
            || lower.contains("cat >") || lower.contains("tee ") || lower.contains("rm ") {
            return Err("BLOCKED: cannot modify kubeconfig".into());
        }
    }

    // ── Block process-level env mutation (ptrace, gdb) ──
    for tool in &["gdb ", "strace ", "ltrace ", "ptrace"] {
        if lower.contains(tool) {
            return Err(format!(
                "BLOCKED: process inspection tool not allowed ({})",
                tool.trim()
            ));
        }
    }

    Ok(())
}

/// Check if a file write path is protected.
pub fn check_file_write_safety(path: &str) -> Result<(), String> {
    let protected_prefixes = [
        "/home/jcode/.ohagent",
        "/etc/kubernetes",
        "/var/run/secrets",
        "/proc/1",
        "/vault",
        "/usr/local/bin/ohagent-daemon",
        "/.dockerenv",
        "/etc/ssh",
        "/etc/ssl",
        "/etc/pam.d",
    ];

    let normalized = if path.starts_with('~') {
        path.replacen('~', "/home/jcode", 1)
    } else {
        path.to_string()
    };

    for prefix in &protected_prefixes {
        if normalized.starts_with(prefix) {
            return Err(format!(
                "BLOCKED: cannot write to protected path {} (prefix: {})",
                path, prefix
            ));
        }
    }

    Ok(())
}

fn is_mutation(command: &str, tool: &str) -> bool {
    let after = command.split(tool).nth(1).unwrap_or("");
    let after = after.trim();
    // Block start, stop, restart, disable, mask, enable, edit
    let mutation_verbs = ["start ", "stop ", "restart ", "disable ", "mask ", "enable ", "edit "];
    mutation_verbs.iter().any(|v| after.starts_with(v))
        || !["status ", "list-units ", "is-active ", "show "].iter().any(|v| after.starts_with(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kubectl_read_allowed() {
        assert!(check_command_safety("kubectl get pods", "/tmp").is_ok());
        assert!(check_command_safety("kubectl describe pod foo", "/tmp").is_ok());
        assert!(check_command_safety("kubectl logs deploy/ohagent-daemon", "/tmp").is_ok());
    }

    #[test]
    fn test_kubectl_write_blocked() {
        assert!(check_command_safety("kubectl apply -f foo.yaml", "/tmp").is_err());
        assert!(check_command_safety("kubectl delete pod x", "/tmp").is_err());
        assert!(check_command_safety("kubectl patch deploy x", "/tmp").is_err());
        assert!(check_command_safety("kubectl delete configmap ohagent-config", "/tmp").is_err());
        assert!(check_command_safety("kubectl set env deploy/ohagent-daemon FOO=bar", "/tmp").is_err());
        assert!(check_command_safety("kubectl create secret generic x", "/tmp").is_err());
    }

    #[test]
    fn test_env_protection() {
        assert!(check_command_safety("export DEEPSEEK_API_KEY=hack", "/tmp").is_err());
        assert!(check_command_safety("setenv TELEGRAM_BOT_TOKEN hack", "/tmp").is_err());
        assert!(check_command_safety("set VAULT_TOKEN=hack", "/tmp").is_err());
    }

    #[test]
    fn test_protected_paths() {
        assert!(check_command_safety("echo hack > /home/jcode/.ohagent/keys.toml", "/tmp").is_err());
        assert!(check_command_safety("rm -rf /home/jcode/.ohagent", "/tmp").is_err());
        assert!(check_command_safety("cp /tmp/x /etc/kubernetes/manifests/pwn.yaml", "/tmp").is_err());
    }

    #[test]
    fn test_helm_docker_blocked() {
        assert!(check_command_safety("helm install x", "/tmp").is_err());
        assert!(check_command_safety("docker run alpine", "/tmp").is_err());
        assert!(check_command_safety("docker build .", "/tmp").is_err());
        assert!(check_command_safety("docker ps", "/tmp").is_ok());  // read-only allowed
    }

    #[test]
    fn test_file_write_check() {
        assert!(check_file_write_safety("/tmp/test.txt").is_ok());
        assert!(check_file_write_safety("/home/jcode/test.txt").is_ok());
        assert!(check_file_write_safety("/home/jcode/.ohagent/keys.toml").is_err());
        assert!(check_file_write_safety("~/.ohagent/skills.db").is_err());
        assert!(check_file_write_safety("/etc/kubernetes/manifest.yaml").is_err());
        assert!(check_file_write_safety("/var/run/secrets/token").is_err());
    }

    #[test]
    fn test_systemctl_blocked() {
        assert!(check_command_safety("systemctl stop ohagent-daemon", "/tmp").is_err());
        assert!(check_command_safety("systemctl restart foo", "/tmp").is_err());
        assert!(check_command_safety("systemctl status foo", "/tmp").is_ok());  // read allowed
    }

    #[test]
    fn test_ptrace_blocked() {
        assert!(check_command_safety("strace -p 1", "/tmp").is_err());
        assert!(check_command_safety("gdb -p 1", "/tmp").is_err());
    }
}
