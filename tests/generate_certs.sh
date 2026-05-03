# Test-suite certificate generation
# 

# Terminology:
#   CSR (Certificate Signing Request) 
#   CA (Certificate Authority)
#   PEM (Privacy-Enhanced Mail)
#   TLS (Transport Layer Security)
#   mTLS (Mutual Transport Layer Security)
# 

# Notes:
#   What is a certificate? 
#       Certificate is a public key, with additional metadata indicating it was signed by the CA.
#   
#   Who are the actors in mTLS? What are their roles?
#       Certificate Authority: 
#           - They hold onto the cert and the key. Their key is used to sign other certificates. 
#           - The certificate is the trust anchor that gets distributed.
#       Server:
#           - Holds onto a cert signed by the CA. It's presented to clients during the TLS handshake.
#           - Its key is used to sign handshake messages.
#       Client:
#           - Holds onto a cert and key. The server demands a cert from the client.
# 
#   What is the TLS handshake mechanism?
#       - Server presents certificate, client ensures its valid.
#       - In mutual TLS, they both have to present a valid certifiacte.

# Purpose of the script:
#   - Idempotent generation of certificates to test verification. 


# Early exit for idempotency
if [ -f tests/certs/ca-cert.pem]; then exit 0; fi



# Generate self-signed cert via OpenSSL
# 
# openssl req:
#   x509: Output an X.509 certificate structure instead of a cert request
#   newkey {val}: Generate new key with [<alg>:]<nbits> or <alg>[:<file>] or param:<file>
#   keyout {outfile}: File to write private key to
#   out {outfile}: Output file
#   days {+int}: Number of days certificate is valid for
#   noenc: Don't encrypt private keys
#   subj {val}: Set or modify subject of request or cert
openssl req \
    -x509 \
    -newkey rsa:2048 \
    -keyout key.pem \
    -out cert.pem \
    -days 365 \
    -noenc \
    -subj 

# Server cert: needs a CSR that the CA signs. Will be loaded by the test suite server.
#
# Generate server key
openssl req \
    -new \
    -newkey ... \
    -keyout server-key.pem \
    -out server.csr \
    -nodes \
    -subj ...

# Sign CSR:
openssl x509 \
    -req \
    -in server.csr \
    -CA <ca cert> \
    -CAkey <ca key> \
    -CAcreateserial \
    -out server-cert.pem \
    -days ...

# Client cert: Same as server cert, signed by same CA. 

# Create combined pem
cat client-cert.pem client-key.pem > client-combined.pem