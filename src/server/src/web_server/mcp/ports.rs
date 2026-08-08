//! Port deny-list for preview security.
//!
//! Well-known service ports must never be opened as browser Preview targets —
//! a dev agent pointing Preview at e.g. Postgres or SSH would expose a
//! sensitive local service to browser traffic.

/// Well-known service ports that must not be opened as Preview targets.
const DENIED_PORTS: &[u16] = &[
    22,    // SSH
    25,    // SMTP
    53,    // DNS
    110,   // POP3
    143,   // IMAP
    389,   // LDAP
    443,   // HTTPS (typically reverse proxy)
    445,   // SMB
    993,   // IMAPS
    995,   // POP3S
    1433,  // MSSQL
    1521,  // Oracle
    2049,  // NFS
    3306,  // MySQL
    5432,  // PostgreSQL
    5672,  // RabbitMQ
    6379,  // Redis
    6380,  // Redis TLS
    9200,  // Elasticsearch
    11211, // Memcached
    27017, // MongoDB
];

pub(super) fn is_denied_port(port: u16) -> bool {
    DENIED_PORTS.contains(&port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denies_database_ports() {
        assert!(is_denied_port(3306), "MySQL");
        assert!(is_denied_port(5432), "PostgreSQL");
        assert!(is_denied_port(27017), "MongoDB");
        assert!(is_denied_port(6379), "Redis");
    }

    #[test]
    fn denies_infrastructure_ports() {
        assert!(is_denied_port(22), "SSH");
        assert!(is_denied_port(25), "SMTP");
        assert!(is_denied_port(443), "HTTPS");
    }

    #[test]
    fn allows_typical_dev_server_ports() {
        assert!(!is_denied_port(3000), "common node dev port");
        assert!(!is_denied_port(5173), "vite default");
        assert!(!is_denied_port(5181), "custom vite port");
        assert!(!is_denied_port(8080), "common alt HTTP");
        assert!(!is_denied_port(8000), "python/django");
        assert!(!is_denied_port(4200), "angular");
    }
}
