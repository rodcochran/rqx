use pyo3::create_exception;

/*

Exception Hierarchy to provide a drop-in replacement interface for Httpx

reqx.ReqxError
├── reqx.RequestError
│   ├── reqx.TransportError
│   │   ├── reqx.TimeoutException
│   │   │   ├── reqx.ConnectTimeout
│   │   │   ├── reqx.ReadTimeout
│   │   │   ├── reqx.WriteTimeout
│   │   │   └── reqx.PoolTimeout
│   │   ├── reqx.NetworkError
│   │   │   ├── reqx.ConnectError
│   │   │   ├── reqx.ReadError
│   │   │   └── reqx.WriteError
│   │   ├── reqx.TooManyRedirects
│   │   └── reqx.ProxyError
│   └── reqx.HTTPStatusError          (raised by raise_for_status())
└── reqx.MaxRetriesExceeded           (raised when retry budget exhausted)
*/


// Level 1
create_exception!(reqx, ReqxError, pyo3::exceptions::PyException);

// Level 2
create_exception!(reqx, RequestError, ReqxError);
create_exception!(reqx, MaxRetriesExceeded, ReqxError);

// Level 3
create_exception!(reqx, TransportError, RequestError);
create_exception!(reqx, HTTPStatusError, RequestError);
create_exception!(reqx, TooManyRedirects, RequestError);

// Level 4
create_exception!(reqx, TimeoutException, TransportError);
create_exception!(reqx, NetworkError, TransportError);
create_exception!(reqx, ProxyError, TransportError);

// Level 5
create_exception!(reqx, ConnectTimeout, TimeoutException);
create_exception!(reqx, ReadTimeout, TimeoutException);
create_exception!(reqx, WriteTimeout, TimeoutException);
create_exception!(reqx, PoolTimeout, TimeoutException);
create_exception!(reqx, ConnectError, NetworkError);
create_exception!(reqx, ReadError, NetworkError);
create_exception!(reqx, WriteError, NetworkError);



