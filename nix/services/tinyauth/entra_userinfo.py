"""Userinfo endpoint for tinyauth's Entra ID provider.

Tinyauth reads user claims only from the provider's userinfo URL. Entra's own
userinfo (graph.microsoft.com/oidc/userinfo) has no groups, and no email for
users without a mailbox. This service takes the Graph access token tinyauth
forwards, asks Microsoft Graph for the user and their (transitive) groups, and
answers with flat OIDC-style claims:

    {"sub", "name", "preferred_username", "email", "groups": [...]}

`groups` holds each group's display name (when readable) and its object ID, so
neo access rules may use either. Stdlib only; runs in a plain python image.
"""

import json
import os
import sys
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

GRAPH = os.environ.get("GRAPH_URL", "https://graph.microsoft.com/v1.0")
PORT = int(os.environ.get("PORT", "8080"))
TIMEOUT = 15


class GraphError(Exception):
    def __init__(self, status, body):
        super().__init__(f"graph {status}: {body[:300]}")
        self.status = status


def graph_get(url, token):
    req = urllib.request.Request(
        url,
        headers={"Authorization": f"Bearer {token}", "Accept": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as res:
            return json.load(res)
    except urllib.error.HTTPError as e:
        raise GraphError(e.code, e.read().decode("utf-8", "replace")) from e


def user_groups(token):
    groups = []
    url = (
        f"{GRAPH}/me/transitiveMemberOf?$select=id,displayName&$top=999"
    )
    while url:
        page = graph_get(url, token)
        for obj in page.get("value", []):
            if obj.get("@odata.type") != "#microsoft.graph.group":
                continue
            for value in (obj.get("displayName"), obj.get("id")):
                if value and value not in groups:
                    groups.append(value)
        url = page.get("@odata.nextLink")
    return groups


def claims(token):
    me = graph_get(
        f"{GRAPH}/me?$select=id,displayName,mail,userPrincipalName", token
    )
    upn = me.get("userPrincipalName") or ""
    return {
        "sub": me.get("id") or "",
        "name": me.get("displayName") or upn,
        "preferred_username": upn,
        # Users without a mailbox have no `mail`; the UPN is their sign-in name.
        "email": (me.get("mail") or upn).lower(),
        "groups": user_groups(token),
    }


class Handler(BaseHTTPRequestHandler):
    server_version = "neo-entra-userinfo"

    def _send(self, status, body):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path == "/health":
            self._send(200, {"status": "ok"})
            return
        auth = self.headers.get("Authorization", "")
        scheme, _, token = auth.partition(" ")
        if scheme.lower() != "bearer" or not token:
            self._send(401, {"error": "missing bearer token"})
            return
        try:
            body = claims(token)
        except GraphError as e:
            print(f"entra-userinfo: {e}", file=sys.stderr, flush=True)
            self._send(e.status if e.status in (401, 403) else 502, {"error": str(e)})
            return
        except (urllib.error.URLError, TimeoutError, ValueError) as e:
            print(f"entra-userinfo: graph unreachable: {e}", file=sys.stderr, flush=True)
            self._send(502, {"error": "graph unreachable"})
            return
        print(
            f"entra-userinfo: {body['email']} groups={len(body['groups'])}",
            file=sys.stderr,
            flush=True,
        )
        self._send(200, body)

    do_POST = do_GET

    def log_message(self, fmt, *args):
        pass


if __name__ == "__main__":
    print(f"entra-userinfo: listening on :{PORT}", file=sys.stderr, flush=True)
    ThreadingHTTPServer(("0.0.0.0", PORT), Handler).serve_forever()
