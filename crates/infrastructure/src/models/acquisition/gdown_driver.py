import json
import sys
import time

TAG = "@vj-gdown "


def emit(stream, value):
    stream.write(TAG + json.dumps(value, ensure_ascii=True, separators=(",", ":")) + "\n")
    stream.flush()


def kind(error):
    try:
        import requests
        from gdown.exceptions import DownloadError, FileURLRetrievalError
    except ImportError:
        return "other"
    if isinstance(error, FileURLRetrievalError):
        text = str(error).lower()
        if "too many users" in text or "quota" in text or "at this time" in text:
            return "quota"
        return "permission"
    if isinstance(error, requests.exceptions.HTTPError):
        status = error.response.status_code if error.response is not None else 0
        if status in (401, 403):
            return "permission"
        if status in (404, 410):
            return "not_found"
        if status == 429 or status >= 500:
            return "service"
        return "other"
    if isinstance(error, (requests.exceptions.ConnectionError, requests.exceptions.Timeout,
                          requests.exceptions.ChunkedEncodingError)):
        return "network"
    if isinstance(error, requests.exceptions.RequestException):
        return "other"
    if isinstance(error, DownloadError):
        if str(error).startswith(("Download failed after", "Download is incomplete",
                                  "Server did not honor", "Response exceeds")):
            return "network"
        return "other"
    if isinstance(error, OSError):
        return "disk"
    return "other"


def main():
    mode, file_id = sys.argv[1], sys.argv[2]
    try:
        import gdown

        url = "https://drive.google.com/uc?id=" + file_id
        if mode == "resolve":
            found = gdown.download(url, quiet=True, use_cookies=False, skip_download=True, timeout=30)
            emit(sys.stdout, {"name": found.path})
        elif mode == "download":
            last = [0.0]

            def progress(done, total):
                now = time.monotonic()
                if now - last[0] >= 0.25:
                    last[0] = now
                    emit(sys.stdout, {"done": done, "total": total})

            gdown.download(url, sys.argv[3], quiet=True, use_cookies=False, resume=False,
                           progress=progress, timeout=60, retries=3)
        else:
            raise ValueError("unknown mode")
    except Exception as error:
        emit(sys.stderr, {"error": kind(error), "detail": (type(error).__name__ + ": " + str(error))[:500]})
        sys.exit(1)


main()
