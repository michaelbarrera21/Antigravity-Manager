# ECDSA Root CA 证书生成

本目录包含用于生成 ECDSA Root CA 证书的脚本，支持 P-256 和 P-384 曲线。

## 使用方法

### Linux/macOS

```bash
# 使用默认参数生成 P-256 证书
./generate_ecdsa_ca.sh

# 生成 P-384 证书
./generate_ecdsa_ca.sh --curve secp384r1

# 生成 20 年有效期的证书
./generate_ecdsa_ca.sh --days 7300

# 指定输出目录
./generate_ecdsa_ca.sh --output-dir /path/to/output
```

### Windows

```cmd
REM 使用默认参数生成 P-256 证书
generate_ecdsa_ca.bat

REM 生成 P-384 证书
generate_ecdsa_ca.bat --curve secp384r1

REM 生成 20 年有效期的证书
generate_ecdsa_ca.bat --days 7300

REM 指定输出目录
generate_ecdsa_ca.bat --output-dir C:\path\to\output
```

## 参数说明

| 参数 | 说明 | 默认值 |
|------|------|--------|
| `--curve` | 椭圆曲线类型 (prime256v1/secp384r1) | prime256v1 |
| `--days` | 证书有效期天数 | 3650 |
| `--output-dir` | 输出目录 | 当前目录 |
| `-h, --help` | 显示帮助信息 | - |

## 输出文件

- `ca-key.pem` - ECDSA 私钥
- `ca-cert.pem` - Root CA 证书

## 证书安装

生成证书后，需要将其安装到系统受信任的根证书颁发机构：

### Windows (管理员权限)
```cmd
certutil -addstore Root "ca-cert.pem"
```

### macOS
```bash
sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain "ca-cert.pem"
```

### Linux
```bash
sudo cp "ca-cert.pem" /usr/local/share/ca-certificates/antigravity-ca.crt
sudo update-ca-certificates
```

## 配置 Antigravity

1. 在 Antigravity 中设置 Root CA 证书和私钥路径
2. 启动 MITM 代理
3. 配置应用程序使用代理地址: `127.0.0.1:8081`

## 注意事项

- 仅支持 ECDSA P-256 (prime256v1) 和 P-384 (secp384r1) 曲线
- 不支持 RSA 密钥
- 证书默认有效期 10 年 (3650 天)
- 确保系统已安装 OpenSSL 工具
