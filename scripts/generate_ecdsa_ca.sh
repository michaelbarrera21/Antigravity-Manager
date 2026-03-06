#!/bin/bash
# ECDSA Root CA 证书生成脚本
# 仅支持 ECDSA P-256/P-384

set -e

echo "=== ECDSA Root CA 证书生成脚本 ==="
echo ""

# 默认参数
CURVE="prime256v1"  # P-256
DAYS=3650
OUTPUT_DIR="."

# 解析参数
while [[ $# -gt 0 ]]; do
    case $1 in
        --curve)
            CURVE="$2"
            shift 2
            ;;
        --days)
            DAYS="$2"
            shift 2
            ;;
        --output-dir)
            OUTPUT_DIR="$2"
            shift 2
            ;;
        -h|--help)
            echo "用法: $0 [选项]"
            echo ""
            echo "选项:"
            echo "  --curve CURVE    椭圆曲线 (prime256v1|secp384r1)，默认: prime256v1"
            echo "  --days DAYS       有效期天数，默认: 3650"
            echo "  --output-dir DIR  输出目录，默认: 当前目录"
            echo "  -h, --help       显示帮助信息"
            echo ""
            echo "示例:"
            echo "  $0                           # 使用默认参数生成 P-256 证书"
            echo "  $0 --curve secp384r1        # 生成 P-384 证书"
            echo "  $0 --days 7300               # 生成 20 年有效期的证书"
            exit 0
            ;;
        *)
            echo "未知参数: $1"
            echo "使用 -h 或 --help 查看帮助"
            exit 1
            ;;
    esac
done

# 验证曲线类型
if [[ "$CURVE" != "prime256v1" && "$CURVE" != "secp384r1" ]]; then
    echo "错误: 不支持的曲线类型: $CURVE"
    echo "支持的曲线: prime256v1 (P-256), secp384r1 (P-384)"
    exit 1
fi

# 创建输出目录
mkdir -p "$OUTPUT_DIR"

# 生成私钥
echo "1. 生成 ECDSA 私钥 (曲线: $CURVE)..."
openssl ecparam -genkey -name "$CURVE" -noout -out "$OUTPUT_DIR/ca-key.pem"
echo "   私钥已保存到: $OUTPUT_DIR/ca-key.pem"

# 生成证书
echo "2. 生成 Root CA 证书..."
openssl req -new -x509 -key "$OUTPUT_DIR/ca-key.pem" -out "$OUTPUT_DIR/ca-cert.pem" -days "$DAYS" -subj '/CN=Antigravity-MITM-CA'
echo "   证书已保存到: $OUTPUT_DIR/ca-cert.pem"

# 验证文件
echo ""
echo "3. 验证生成的文件..."
if [[ -f "$OUTPUT_DIR/ca-key.pem" && -f "$OUTPUT_DIR/ca-cert.pem" ]]; then
    echo "   ✓ 私钥文件存在"
    echo "   ✓ 证书文件存在"
    
    # 显示证书信息
    echo ""
    echo "4. 证书信息:"
    openssl x509 -in "$OUTPUT_DIR/ca-cert.pem" -text -noout | grep -E "(Subject:|Not Before:|Not After:|Public Key Algorithm:|Signature Algorithm:)"
    
    echo ""
    echo "=== 生成完成 ==="
    echo "私钥: $OUTPUT_DIR/ca-key.pem"
    echo "证书: $OUTPUT_DIR/ca-cert.pem"
    echo ""
    echo "下一步:"
    echo "1. 将证书安装到系统受信任的根证书颁发机构"
    echo "2. 在 Antigravity 中配置 MITM 代理"
    echo "3. 使用代理地址: 127.0.0.1:8081"
else
    echo "   ✗ 文件生成失败"
    exit 1
fi
