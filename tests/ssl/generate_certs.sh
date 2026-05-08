# Test-suite certificate generation
# 

# Terminology:
#   CSR (Certificate Signing Request) 
#   CA (Certificate Authority)
#   PEM (Privacy-Enhanced Mail)
#   TLS (Transport Layer Security)
#   mTLS (Mutual Transport Layer Security)
#   SSL (Secure Sockets Layer)
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

SCRIPT_DIR="$(dirname "${BASH_SOURCE[0]}")"
CERTS_DIR="$SCRIPT_DIR/certs"

# # Early exit for idempotency
# if [ -f $CERTS_DIR/client-cert.pem ]; 
#     then 
#     echo "Certs already created - exiting"
#     exit 0; 
# fi

echo "Generating Certificates"
mkdir -p $CERTS_DIR

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
    -keyout $CERTS_DIR/key.pem \
    -out $CERTS_DIR/cert.pem \
    -days 1 \
    -noenc \
    -subj /C=US/ST=CA/L="San Francisco"/O=rqx/ \
    -quiet

echo "Generated self-signed cert"

# Server cert: needs a CSR that the CA signs. Will be loaded by the test suite server.
#
# Generate server key
openssl req \
    -new \
    -newkey rsa:2048 \
    -keyout $CERTS_DIR/server-key.pem \
    -out $CERTS_DIR/server.csr \
    -noenc \
    -subj /C=US/ST=CA/L="San Francisco"/O="rqx Test Server"/ \
    -quiet


echo "Generated server key"

# Sign CSR:
openssl x509 \
    -req \
    -in $CERTS_DIR/server.csr \
    -CA $CERTS_DIR/cert.pem \
    -CAkey $CERTS_DIR/key.pem \
    -CAcreateserial \
    -out $CERTS_DIR/server-cert.pem \
    -days 1 \
    -extfile $SCRIPT_DIR/extfile.txt

    

echo "Signed Server CSR"

# Client cert: Same as server cert, signed by same CA. 
#
# Generate Client key
openssl req \
    -new \
    -newkey rsa:2048 \
    -keyout $CERTS_DIR/client-key.pem \
    -out $CERTS_DIR/client.csr \
    -noenc \
    -subj /C=US/ST=CA/L="San Francisco"/O="rqx Test Client"/ \
    -quiet

echo "Generated client key"

# Sign CSR:
openssl x509 \
    -req \
    -in $CERTS_DIR/client.csr \
    -CA $CERTS_DIR/cert.pem \
    -CAkey $CERTS_DIR/key.pem \
    -CAcreateserial \
    -out $CERTS_DIR/client-cert.pem \
    -days 1

echo "Signed Client CSR"

# Create combined pem
cat $CERTS_DIR/client-cert.pem $CERTS_DIR/client-key.pem > $CERTS_DIR/client-combined.pem

echo "Combined PEMs to $CERTS_DIR/client-combined.pem"